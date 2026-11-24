//! https://drafts.csswg.org/css-syntax-3/#parsing
//!
//! the input is first turned into a list of component values, and the rule and declaration
//! algorithms run over that list: a {}-block stands in for the <{-token> that starts it, and the
//! end of its value for the <}-token> that ends it. every algorithm that can meet a <}-token>
//! inside a block stops there without consuming it, exactly as it stops at eof, so the result is
//! the same as running them over the token stream.

use std::borrow::Cow;

use super::tokenizer::{Tokenizer, preprocess};
use super::{
    AtRule, BlockItem, BlockKind, ComponentValue, Declaration, Function, ParseError,
    ParseErrorKind, QualifiedRule, Rule, SimpleBlock, Span, Stylesheet, SyntaxError, Token,
};

/// functions and simple blocks nested deeper than this are dropped, so that hostile input can't
/// overflow the stack in the recursive algorithms (or in dropping the tree).
const MAX_NESTING_DEPTH: usize = 128;

enum FrameKind {
    Function(String),
    Block(BlockKind),
}

struct Frame {
    kind: FrameKind,
    start: usize,
    values: Vec<ComponentValue>,
    dropped: bool,
}

impl Frame {
    fn is_closed_by(&self, token: &Token) -> bool {
        matches!(
            (&self.kind, token),
            (
                FrameKind::Function(_) | FrameKind::Block(BlockKind::Paren),
                Token::CloseParen
            ) | (FrameKind::Block(BlockKind::Square), Token::CloseSquare)
                | (FrameKind::Block(BlockKind::Curly), Token::CloseCurly)
        )
    }

    fn finish(self, end: usize) -> ComponentValue {
        let span = Span {
            start: self.start,
            end,
        };
        match self.kind {
            FrameKind::Function(name) => ComponentValue::Function(Function {
                name,
                value: self.values,
                span,
            }),
            FrameKind::Block(kind) => ComponentValue::SimpleBlock(SimpleBlock {
                kind,
                value: self.values,
                span,
            }),
        }
    }
}

fn push_value(stack: &mut [Frame], top: &mut Vec<ComponentValue>, value: ComponentValue) {
    match stack.last_mut() {
        Some(frame) if frame.dropped => {}
        Some(frame) => frame.values.push(value),
        None => top.push(value),
    }
}

/// tokenizes preprocessed `input` and runs
/// https://drafts.csswg.org/css-syntax-3/#consume-list-of-components over the tokens without a
/// stop token and not nested, which consumes all of them. functions and simple blocks are built
/// on an explicit stack
/// (https://drafts.csswg.org/css-syntax-3/#consume-component-value).
pub(crate) fn component_values(
    input: &str,
    offset: usize,
    unicode_ranges_allowed: bool,
    errors: &mut Vec<ParseError>,
) -> Vec<ComponentValue> {
    let mut tokenizer = Tokenizer::new(input, offset, unicode_ranges_allowed, errors);
    let mut stack: Vec<Frame> = Vec::new();
    let mut top = Vec::new();
    while let Some((token, span)) = tokenizer.next_token() {
        let kind = match token {
            Token::Function(name) => FrameKind::Function(name),
            Token::OpenParen => FrameKind::Block(BlockKind::Paren),
            Token::OpenSquare => FrameKind::Block(BlockKind::Square),
            Token::OpenCurly => FrameKind::Block(BlockKind::Curly),
            token => {
                // https://drafts.csswg.org/css-syntax-3/#consume-simple-block and
                // https://drafts.csswg.org/css-syntax-3/#consume-function: the ending token
                // closes the innermost one.
                if stack.last().is_some_and(|frame| frame.is_closed_by(&token)) {
                    if let Some(frame) = stack.pop().filter(|frame| !frame.dropped) {
                        let value = frame.finish(span.end);
                        push_value(&mut stack, &mut top, value);
                    }
                } else {
                    push_value(&mut stack, &mut top, ComponentValue::Token(token, span));
                }
                continue;
            }
        };
        let dropped = stack.len() >= MAX_NESTING_DEPTH;
        stack.push(Frame {
            kind,
            start: span.start,
            values: Vec::new(),
            dropped,
        });
    }
    // eof closes whatever is still open.
    let end = tokenizer.end();
    while let Some(frame) = stack.pop() {
        if !frame.dropped {
            let value = frame.finish(end);
            push_value(&mut stack, &mut top, value);
        }
    }
    top
}

/// https://drafts.csswg.org/css-syntax-3/#css-token-stream over component values. marks are
/// kept by the one caller that needs them.
struct Stream<'a> {
    values: &'a [ComponentValue],
    index: usize,
}

impl<'a> Stream<'a> {
    fn new(values: &'a [ComponentValue]) -> Self {
        Self { values, index: 0 }
    }

    /// the next token; none is the <eof-token>.
    fn next(&self) -> Option<&'a ComponentValue> {
        self.values.get(self.index)
    }

    fn next_token(&self) -> Option<&'a Token> {
        match self.next() {
            Some(ComponentValue::Token(token, _)) => Some(token),
            _ => None,
        }
    }

    fn is_empty(&self) -> bool {
        self.index >= self.values.len()
    }

    fn consume(&mut self) -> Option<&'a ComponentValue> {
        let value = self.next();
        if value.is_some() {
            self.index += 1;
        }
        value
    }

    fn discard(&mut self) {
        self.consume();
    }

    fn discard_whitespace(&mut self) {
        while self.next_token() == Some(&Token::Whitespace) {
            self.discard();
        }
    }

    fn next_start(&self) -> usize {
        self.next()
            .map_or(self.consumed_end(), |value| value.span().start)
    }

    /// the end of the last consumed value.
    fn consumed_end(&self) -> usize {
        self.index
            .checked_sub(1)
            .and_then(|index| self.values.get(index))
            .map_or(0, |value| value.span().end)
    }
}

fn is_whitespace(value: &ComponentValue) -> bool {
    matches!(value, ComponentValue::Token(Token::Whitespace, _))
}

fn is_curly_block(value: &ComponentValue) -> bool {
    matches!(
        value,
        ComponentValue::SimpleBlock(SimpleBlock {
            kind: BlockKind::Curly,
            ..
        })
    )
}

fn trim_trailing_whitespace(mut values: &[ComponentValue]) -> &[ComponentValue] {
    while let Some((last, rest)) = values.split_last() {
        if !is_whitespace(last) {
            break;
        }
        values = rest;
    }
    values
}

/// the parser algorithms, with the preprocessed source the component values' spans point into.
struct Consumer<'s, 'e> {
    source: &'s str,
    errors: &'e mut Vec<ParseError>,
}

impl Consumer<'_, '_> {
    fn invalid(&mut self, position: usize) {
        self.errors.push(ParseError {
            kind: ParseErrorKind::Invalid,
            position,
        });
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-stylesheet-contents
    fn consume_stylesheet_contents(&mut self, input: &mut Stream<'_>) -> Vec<Rule> {
        let mut rules = Vec::new();
        loop {
            match input.next_token() {
                Some(Token::Whitespace | Token::Cdo | Token::Cdc) => input.discard(),
                Some(Token::AtKeyword(_)) => {
                    rules.push(Rule::At(self.consume_at_rule(input, false)))
                }
                _ if input.is_empty() => return rules,
                _ => {
                    let start = input.next_start();
                    match self.consume_qualified_rule(input, None, false) {
                        Some(rule) => rules.push(Rule::Qualified(rule)),
                        None => self.invalid(start),
                    }
                }
            }
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-at-rule
    /// "valid in the current context" is the grammar's call, so every at-rule is returned.
    fn consume_at_rule(&mut self, input: &mut Stream<'_>, nested: bool) -> AtRule {
        let start = input.next_start();
        let name = match input.consume() {
            Some(ComponentValue::Token(Token::AtKeyword(name), _)) => name.clone(),
            _ => String::new(),
        };
        let mut prelude = Vec::new();
        let block = loop {
            match input.next() {
                None | Some(ComponentValue::Token(Token::Semicolon, _)) => {
                    input.discard();
                    break None;
                }
                Some(ComponentValue::Token(Token::CloseCurly, _)) if nested => break None,
                Some(ComponentValue::SimpleBlock(block)) if block.kind == BlockKind::Curly => {
                    input.discard();
                    break Some(self.consume_block(block));
                }
                Some(value) => {
                    input.discard();
                    prelude.push(value.clone());
                }
            }
        };
        AtRule {
            name,
            prelude,
            block,
            span: Span {
                start,
                end: input.consumed_end(),
            },
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-qualified-rule
    /// every rule is valid at this level, so the invalid rule error never comes back.
    fn consume_qualified_rule(
        &mut self,
        input: &mut Stream<'_>,
        stop_token: Option<&Token>,
        nested: bool,
    ) -> Option<QualifiedRule> {
        let start = input.next_start();
        let mut prelude = Vec::new();
        loop {
            match input.next() {
                None => return None,
                Some(ComponentValue::Token(token, _)) if Some(token) == stop_token => return None,
                Some(ComponentValue::Token(Token::CloseCurly, _)) if nested => return None,
                Some(ComponentValue::SimpleBlock(block)) if block.kind == BlockKind::Curly => {
                    if looks_like_custom_property(&prelude) {
                        if nested {
                            self.consume_remnants_of_bad_declaration(input, true);
                        } else {
                            input.discard();
                        }
                        return None;
                    }
                    input.discard();
                    let mut child_rules = self.consume_block(block).into_iter().peekable();
                    let declarations = match child_rules
                        .next_if(|item| matches!(item, BlockItem::Declarations(_)))
                    {
                        Some(BlockItem::Declarations(declarations)) => declarations,
                        _ => Vec::new(),
                    };
                    let child_rules = child_rules
                        .map(|item| match item {
                            BlockItem::Rule(rule) => rule,
                            BlockItem::Declarations(declarations) => {
                                Rule::NestedDeclarations(declarations)
                            }
                        })
                        .collect();
                    return Some(QualifiedRule {
                        prelude,
                        declarations,
                        child_rules,
                        span: Span {
                            start,
                            end: input.consumed_end(),
                        },
                    });
                }
                Some(value) => {
                    input.discard();
                    prelude.push(value.clone());
                }
            }
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-block
    fn consume_block(&mut self, block: &SimpleBlock) -> Vec<BlockItem> {
        self.consume_block_contents(&mut Stream::new(&block.value))
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-block-contents
    fn consume_block_contents(&mut self, input: &mut Stream<'_>) -> Vec<BlockItem> {
        let mut rules = Vec::new();
        let mut decls = Vec::new();
        let flush = |rules: &mut Vec<BlockItem>, decls: &mut Vec<Declaration>| {
            if !decls.is_empty() {
                rules.push(BlockItem::Declarations(std::mem::take(decls)));
            }
        };
        loop {
            match input.next_token() {
                Some(Token::Whitespace | Token::Semicolon) => input.discard(),
                Some(Token::CloseCurly) => break,
                _ if input.is_empty() => break,
                Some(Token::AtKeyword(_)) => {
                    flush(&mut rules, &mut decls);
                    let rule = self.consume_at_rule(input, true);
                    rules.push(BlockItem::Rule(Rule::At(rule)));
                }
                _ => {
                    let start = input.next_start();
                    let mark = input.index;
                    if let Some(decl) = self.consume_declaration(input, true) {
                        decls.push(decl);
                        continue;
                    }
                    input.index = mark;
                    match self.consume_qualified_rule(input, Some(&Token::Semicolon), true) {
                        Some(rule) => {
                            flush(&mut rules, &mut decls);
                            rules.push(BlockItem::Rule(Rule::Qualified(rule)));
                        }
                        None => self.invalid(start),
                    }
                }
            }
        }
        // the spec returns rules without appending a non-empty decls, which would drop the
        // declarations of every block that doesn't end in a rule. they're appended here.
        flush(&mut rules, &mut decls);
        rules
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-declaration
    /// "valid in the current context" is the grammar's call, so every declaration that gets
    /// through the syntax checks is returned.
    fn consume_declaration(&mut self, input: &mut Stream<'_>, nested: bool) -> Option<Declaration> {
        let start = input.next_start();
        // step 1
        let name = match input.next_token() {
            Some(Token::Ident(name)) => {
                input.discard();
                name.clone()
            }
            _ => {
                self.consume_remnants_of_bad_declaration(input, nested);
                return None;
            }
        };
        // step 2
        input.discard_whitespace();
        // step 3
        if input.next_token() != Some(&Token::Colon) {
            self.consume_remnants_of_bad_declaration(input, nested);
            return None;
        }
        input.discard();
        // step 4
        input.discard_whitespace();
        // step 5
        let list = consume_list_of_component_values(input, Some(&Token::Semicolon), nested);
        // step 6: everything after the "!" other than "important" is whitespace, which step 7
        // trims anyway.
        let mut value = list;
        let mut important = false;
        let mut non_whitespace = list
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, value)| !is_whitespace(value));
        if let (Some((_, last_value)), Some((bang, bang_value))) =
            (non_whitespace.next(), non_whitespace.next())
            && matches!(bang_value, ComponentValue::Token(Token::Delim('!'), _))
            && matches!(last_value, ComponentValue::Token(Token::Ident(ident), _) if ident.eq_ignore_ascii_case("important"))
        {
            value = &list[..bang];
            important = true;
        }
        // step 7
        let value = trim_trailing_whitespace(value);
        // step 8
        let mut original_text = None;
        let mut owned_value = None;
        if name.starts_with("--") {
            original_text = Some(self.segment(value).to_owned());
        } else if value.iter().any(is_curly_block)
            && value.iter().filter(|value| !is_whitespace(value)).count() > 1
        {
            return None;
        } else if name.eq_ignore_ascii_case("unicode-range") {
            owned_value = Some(self.consume_unicode_range_value(list));
        }
        // step 9
        Some(Declaration {
            name,
            value: owned_value.unwrap_or_else(|| value.to_vec()),
            important,
            original_text,
            span: Span {
                start,
                end: input.consumed_end(),
            },
        })
    }

    /// the segment of the source the values were parsed from.
    fn segment(&self, values: &[ComponentValue]) -> &str {
        match (values.first(), values.last()) {
            (Some(first), Some(last)) => self
                .source
                .get(first.span().start..last.span().end)
                .unwrap_or_default(),
            _ => "",
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-unicode-range-value
    fn consume_unicode_range_value(&mut self, values: &[ComponentValue]) -> Vec<ComponentValue> {
        let offset = values.first().map_or(0, |value| value.span().start);
        // errors were reported when the segment was first tokenized.
        component_values(self.segment(values), offset, true, &mut Vec::new())
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-the-remnants-of-a-bad-declaration
    fn consume_remnants_of_bad_declaration(&mut self, input: &mut Stream<'_>, nested: bool) {
        loop {
            match input.next_token() {
                Some(Token::Semicolon) => {
                    input.discard();
                    return;
                }
                Some(Token::CloseCurly) if nested => return,
                _ if input.is_empty() => return,
                _ => input.discard(),
            }
        }
    }
}

/// whether the first two non-whitespace values are an <ident-token> starting with "--" and a
/// <colon-token> (https://drafts.csswg.org/css-syntax-3/#consume-qualified-rule).
fn looks_like_custom_property(prelude: &[ComponentValue]) -> bool {
    let mut values = prelude.iter().filter(|value| !is_whitespace(value));
    matches!(
        (values.next(), values.next()),
        (
            Some(ComponentValue::Token(Token::Ident(name), _)),
            Some(ComponentValue::Token(Token::Colon, _))
        ) if name.starts_with("--")
    )
}

/// https://drafts.csswg.org/css-syntax-3/#consume-list-of-components
fn consume_list_of_component_values<'a>(
    input: &mut Stream<'a>,
    stop_token: Option<&Token>,
    nested: bool,
) -> &'a [ComponentValue] {
    let start = input.index;
    loop {
        match input.next() {
            None => break,
            Some(ComponentValue::Token(token, _)) if Some(token) == stop_token => break,
            Some(ComponentValue::Token(Token::CloseCurly, _)) if nested => break,
            // a <}-token> when not nested is a parse error, and is consumed like anything else.
            Some(_) => input.discard(),
        }
    }
    &input.values[start..input.index]
}

/// the parser entry points (https://drafts.csswg.org/css-syntax-3/#parser-entry-points).
/// spans and error positions are byte offsets into `preprocess(input)`.
#[derive(Debug, Default)]
pub struct Parser {
    errors: Vec<ParseError>,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// the parse errors seen so far, in order.
    pub fn errors(&self) -> &[ParseError] {
        &self.errors
    }

    pub fn take_errors(&mut self) -> Vec<ParseError> {
        std::mem::take(&mut self.errors)
    }

    /// https://drafts.csswg.org/css-syntax-3/#normalize-into-a-token-stream for a string, then
    /// `f` over the resulting stream.
    fn run<R>(
        &mut self,
        input: &str,
        f: impl FnOnce(&mut Consumer<'_, '_>, &mut Stream<'_>) -> R,
    ) -> R {
        let source: Cow<'_, str> = preprocess(input);
        let values = component_values(&source, 0, false, &mut self.errors);
        let mut consumer = Consumer {
            source: &source,
            errors: &mut self.errors,
        };
        f(&mut consumer, &mut Stream::new(&values))
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-stylesheet
    /// step 1, decoding bytes, is `decode_stylesheet`.
    pub fn parse_stylesheet(&mut self, input: &str) -> Stylesheet {
        Stylesheet {
            rules: self.parse_stylesheet_contents(input),
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-stylesheet-contents
    pub fn parse_stylesheet_contents(&mut self, input: &str) -> Vec<Rule> {
        self.run(input, |consumer, input| {
            consumer.consume_stylesheet_contents(input)
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-list-of-declarations
    pub fn parse_list_of_declarations(&mut self, input: &str) -> Vec<Declaration> {
        self.run(input, |consumer, input| {
            let mut decls = Vec::new();
            while !input.is_empty() {
                input.discard_whitespace();
                match input.next_token() {
                    Some(Token::Semicolon) => input.discard(),
                    Some(Token::AtKeyword(_)) => {
                        consumer.consume_at_rule(input, false);
                    }
                    // consuming a declaration at eof returns nothing.
                    _ if input.is_empty() => break,
                    _ => {
                        let start = input.next_start();
                        match consumer.consume_declaration(input, false) {
                            Some(decl) => decls.push(decl),
                            None => consumer.invalid(start),
                        }
                    }
                }
            }
            decls
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-rule
    pub fn parse_rule(&mut self, input: &str) -> Result<Rule, SyntaxError> {
        self.run(input, |consumer, input| {
            input.discard_whitespace();
            let rule = match input.next_token() {
                _ if input.is_empty() => return Err(SyntaxError::Empty),
                Some(Token::AtKeyword(_)) => Rule::At(consumer.consume_at_rule(input, false)),
                _ => Rule::Qualified(
                    consumer
                        .consume_qualified_rule(input, None, false)
                        .ok_or(SyntaxError::Invalid)?,
                ),
            };
            input.discard_whitespace();
            if input.is_empty() {
                Ok(rule)
            } else {
                Err(SyntaxError::ExtraInput)
            }
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-declaration
    pub fn parse_declaration(&mut self, input: &str) -> Result<Declaration, SyntaxError> {
        self.run(input, |consumer, input| {
            input.discard_whitespace();
            if input.is_empty() {
                return Err(SyntaxError::Empty);
            }
            let value = consumer
                .consume_declaration(input, false)
                .ok_or(SyntaxError::Invalid)?;
            input.discard_whitespace();
            if input.is_empty() {
                Ok(value)
            } else {
                Err(SyntaxError::ExtraInput)
            }
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-component-value
    pub fn parse_component_value(&mut self, input: &str) -> Result<ComponentValue, SyntaxError> {
        self.run(input, |_, input| {
            input.discard_whitespace();
            let value = input.consume().ok_or(SyntaxError::Empty)?.clone();
            input.discard_whitespace();
            if input.is_empty() {
                Ok(value)
            } else {
                Err(SyntaxError::ExtraInput)
            }
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-list-of-component-values
    pub fn parse_list_of_component_values(&mut self, input: &str) -> Vec<ComponentValue> {
        component_values(&preprocess(input), 0, false, &mut self.errors)
    }

    /// https://drafts.csswg.org/css-syntax-3/#parse-comma-separated-list-of-component-values
    pub fn parse_comma_separated_list_of_component_values(
        &mut self,
        input: &str,
    ) -> Vec<Vec<ComponentValue>> {
        self.run(input, |_, input| {
            let mut groups = Vec::new();
            while !input.is_empty() {
                groups.push(
                    consume_list_of_component_values(input, Some(&Token::Comma), false).to_vec(),
                );
                input.discard();
            }
            groups
        })
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-block-contents over the whole input. no
    /// entry point does this; the test suite has a file for it.
    #[cfg(test)]
    pub(crate) fn parse_block_contents(&mut self, input: &str) -> Vec<BlockItem> {
        self.run(input, |consumer, input| {
            consumer.consume_block_contents(input)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(value: &ComponentValue) -> &str {
        match value {
            ComponentValue::Token(Token::Ident(name), _) => name,
            _ => panic!("not an ident: {value:?}"),
        }
    }

    #[test]
    fn nested_rules_and_declarations() {
        let rule = Parser::new()
            .parse_rule("a { color: red; b { x: y } top: 0; @media print { z: w } }")
            .unwrap();
        let Rule::Qualified(rule) = rule else {
            panic!("{rule:?}")
        };
        assert_eq!(rule.declarations.len(), 1);
        assert_eq!(rule.declarations[0].name, "color");
        let [
            Rule::Qualified(b),
            Rule::NestedDeclarations(top),
            Rule::At(media),
        ] = &rule.child_rules[..]
        else {
            panic!("{:?}", rule.child_rules)
        };
        assert_eq!(ident(&b.prelude[0]), "b");
        assert_eq!(b.declarations[0].name, "x");
        assert_eq!(top[0].name, "top");
        assert_eq!(media.name, "media");
        assert!(
            matches!(&media.block.as_deref(), Some([BlockItem::Declarations(d)]) if d[0].name == "z")
        );
    }

    #[test]
    fn declarations_before_and_after_rules_in_blocks() {
        let items = Parser::new().parse_block_contents("a:b; c:hover {} d:e");
        assert!(matches!(
            &items[..],
            [BlockItem::Declarations(a), BlockItem::Rule(Rule::Qualified(_)), BlockItem::Declarations(d)]
                if a[0].name == "a" && d[0].name == "d"
        ));
    }

    #[test]
    fn custom_property_keeps_original_text() {
        let decl = Parser::new()
            .parse_declaration("--x:  { a } /* c */ b !important")
            .unwrap();
        assert!(decl.important);
        assert_eq!(decl.original_text.as_deref(), Some("{ a } /* c */ b"));
        // a non-custom property can't mix a {}-block with anything else.
        assert_eq!(
            Parser::new().parse_declaration("x: {a} b"),
            Err(SyntaxError::Invalid)
        );
        assert!(Parser::new().parse_declaration("x: {a}").is_ok());
    }

    #[test]
    fn custom_property_lookalike_rules_are_dropped() {
        let mut parser = Parser::new();
        assert_eq!(parser.parse_stylesheet("--a:hover { } b {}").rules.len(), 1);
        let items = parser.parse_block_contents("--a: {} x; b: c");
        assert!(matches!(&items[..], [BlockItem::Declarations(d)] if d.len() == 2));
    }

    #[test]
    fn unicode_range_descriptor_retokenizes() {
        let decl = Parser::new()
            .parse_declaration("unicode-range: U+0-7F, u+4??")
            .unwrap();
        let ranges: Vec<_> = decl
            .value
            .iter()
            .filter_map(|value| match value {
                ComponentValue::Token(Token::UnicodeRange { start, end }, _) => {
                    Some((*start, *end))
                }
                _ => None,
            })
            .collect();
        assert_eq!(ranges, [(0, 0x7f), (0x400, 0x4ff)]);
        assert_eq!(decl.value[0].span(), Span { start: 15, end: 21 });
    }

    #[test]
    fn comma_separated_lists() {
        let groups = Parser::new().parse_comma_separated_list_of_component_values("a, (b, c),");
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[1].len(), 2);
    }

    #[test]
    fn deep_nesting_is_dropped_not_overflowed() {
        for (open, close, top_level) in [
            ("(", ")", 1),
            ("[", "]", 1),
            ("{", "}", 1),
            ("f(", ")", 1),
            ("a{", "}", 2),
        ] {
            let input = open.repeat(20_000) + &close.repeat(20_000);
            let mut parser = Parser::new();
            parser.parse_stylesheet(&input);
            parser.parse_list_of_declarations(&input);
            let _ = parser.parse_rule(&input);
            let values = parser.parse_list_of_component_values(&input);
            assert_eq!(values.len(), top_level);
        }
    }

    /// arbitrary input never panics: a fixed pseudo-random byte soup over the characters the
    /// tokenizer treats specially.
    #[test]
    fn arbitrary_input_does_not_panic() {
        let alphabet: Vec<char> =
            "{}()[];:,!@#$%^&*-+._/\\'\"<>=~|?uU0189eEn \n\t\r\u{c}\0é\u{fffd}url"
                .chars()
                .collect();
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        for _ in 0..2000 {
            let len = (state % 64) as usize;
            let input: String = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    alphabet[(state % alphabet.len() as u64) as usize]
                })
                .collect();
            let mut parser = Parser::new();
            parser.parse_stylesheet(&input);
            parser.parse_list_of_declarations(&input);
            parser.parse_block_contents(&input);
            let _ = parser.parse_rule(&input);
            let _ = parser.parse_declaration(&input);
            let _ = parser.parse_component_value(&input);
            parser.parse_comma_separated_list_of_component_values(&input);
            let _ = super::super::parse_a_n_plus_b(&parser.parse_list_of_component_values(&input));
            for error in parser.errors() {
                assert!(error.position <= preprocess(&input).len());
            }
        }
    }
}
