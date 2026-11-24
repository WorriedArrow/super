//! runs tests/css-parsing-tests (https://github.com/SimonSapin/css-parsing-tests), which follows
//! the 2021 candidate recommendation draft of css syntax. where the current draft gives a
//! different result, `DRAFT_RESULTS` holds the current draft's result for that case.
//!
//! the suite's representation of blocks is their component values, while rules here hold their
//! parsed contents, so a rule's block is retokenized from its span for the comparison. dropped
//! rules and declarations appear as `["error", "invalid"]` at their position in the list.

use serde_json::{Value, json};

use super::parser::component_values;
use super::*;

/// (file, input, result under the current draft)
const DRAFT_RESULTS: &[(&str, &str, &str)] = &[
    // u+0080 and u+0081 aren't non-ascii ident code points any more.
    (
        "component_value_list",
        "\\- red0 -red --red -\\-red\\ blue 0red -0red \0red _Red .red rêd r\\êd \u{7f}\u{80}\u{81}",
        r#"[["ident", "-"], " ", ["ident", "red0"], " ", ["ident", "-red"], " ", ["ident", "--red"], " ",
            ["ident", "--red blue"], " ", ["dimension", "0", 0, "integer", "red"], " ",
            ["dimension", "-0", 0, "integer", "red"], " ", ["ident", "�red"], " ", ["ident", "_Red"], " ",
            ".", ["ident", "red"], " ", ["ident", "rêd"], " ", ["ident", "rêd"], " ", "\u007F", "\u0080", "\u0081"]"#,
    ),
    // the current draft has no match or column tokens: they're two delims.
    (
        "component_value_list",
        "~=|=^=$=*=||<!------> |/**/| ~/**/=",
        r#"["~", "=", "|", "=", "^", "=", "$", "=", "*", "=", "|", "|", "<!--", ["ident", "----"], ">", " ", "|", "|", " ", "~", "="]"#,
    ),
    (
        "component_value_list",
        "a:not([href^=http\\:],  [href ^=\t'https\\:'\n]) { color: rgba(0%, 100%, 50%); }",
        r#"[["ident", "a"], ":", ["function", "not", ["[]", ["ident", "href"], "^", "=", ["ident", "http:"]], ",", " ",
            ["[]", ["ident", "href"], " ", "^", "=", " ", ["string", "https:"], " "]], " ",
            ["{}", " ", ["ident", "color"], ":", " ", ["function", "rgba", ["percentage", "0", 0, "integer"], ",", " ",
            ["percentage", "100", 100, "integer"], ",", " ", ["percentage", "50", 50, "integer"]], ";", " "]]"#,
    ),
    // a declaration's value starts after the whitespace following the colon, and trailing
    // whitespace is trimmed after "!important" is removed.
    (
        "one_declaration",
        "\n/**/ foo: ",
        r#"["declaration", "foo", [], false]"#,
    ),
    (
        "one_declaration",
        "foo: 9000  !Important",
        r#"["declaration", "foo", [["number", "9000", 9000, "integer"]], true]"#,
    ),
    (
        "one_declaration",
        "foo: 9000  ! /**/\t IMPORTant /**/\u{c}",
        r#"["declaration", "foo", [["number", "9000", 9000, "integer"]], true]"#,
    ),
    (
        "one_declaration",
        "foo: 9000  /* Dotted capital I */!İmportant",
        r#"["declaration", "foo", [["number", "9000", 9000, "integer"], " ", "!", ["ident", "İmportant"]], false]"#,
    ),
    (
        "one_declaration",
        "foo: 9000  !important!",
        r#"["declaration", "foo", [["number", "9000", 9000, "integer"], " ", "!", ["ident", "important"], "!"], false]"#,
    ),
    (
        "one_declaration",
        "foo: 9000  important",
        r#"["declaration", "foo", [["number", "9000", 9000, "integer"], " ", ["ident", "important"]], false]"#,
    ),
    // consuming a declaration stops at the semicolon, which parse a declaration then finds as
    // extra input.
    ("one_declaration", "foo:;", r#"["error", "extra-input"]"#),
    (
        "one_declaration",
        "foo:;bar:;",
        r#"["error", "extra-input"]"#,
    ),
    // parse a list of declarations discards at-rules.
    (
        "declaration_list",
        "@import 'foo.css'; a:b; @import 'bar.css'",
        r#"[["declaration", "a", [["ident", "b"]], false]]"#,
    ),
    (
        "declaration_list",
        "@media screen { div{;}} a:b;; @media print{div{",
        r#"[["declaration", "a", [["ident", "b"]], false]]"#,
    ),
    (
        "declaration_list",
        "@ media screen { div{;}} a:b;; @media print{div{",
        r#"[["error", "invalid"]]"#,
    ),
    // there is no "parse a list of rules" any more; parse a stylesheet's contents always
    // discards cdo and cdc tokens between rules.
    (
        "rule_list",
        " /* CDO/CDC are not special */ <!-- --> {",
        r#"[["qualified rule", [], []]]"#,
    ),
    (
        "rule_list",
        "div {} -->",
        r#"[["qualified rule", [["ident", "div"], " "], []]]"#,
    ),
];

fn number_repr(text: &str) -> &str {
    let bytes = text.as_bytes();
    let digits = |mut i: usize| {
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let mut end = digits(usize::from(matches!(bytes.first(), Some(b'+' | b'-'))));
    if bytes.get(end) == Some(&b'.') && bytes.get(end + 1).is_some_and(u8::is_ascii_digit) {
        end = digits(end + 1);
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        if bytes.get(end + 1 + sign).is_some_and(u8::is_ascii_digit) {
            end = digits(end + 1 + sign);
        }
    }
    &text[..end]
}

fn numeric(kind: &str, repr: &str, value: &Numeric) -> Vec<Value> {
    let type_flag = if value.is_integer {
        "integer"
    } else {
        "number"
    };
    vec![
        json!(kind),
        json!(number_repr(repr)),
        json!(value.value),
        json!(type_flag),
    ]
}

fn component_value(source: &str, value: &ComponentValue) -> Value {
    let (token, span) = match value {
        ComponentValue::Function(function) => {
            let mut out = vec![json!("function"), json!(function.name)];
            out.extend(
                function
                    .value
                    .iter()
                    .map(|value| component_value(source, value)),
            );
            return Value::Array(out);
        }
        ComponentValue::SimpleBlock(block) => {
            let name = match block.kind {
                BlockKind::Curly => "{}",
                BlockKind::Square => "[]",
                BlockKind::Paren => "()",
            };
            let mut out = vec![json!(name)];
            out.extend(
                block
                    .value
                    .iter()
                    .map(|value| component_value(source, value)),
            );
            return Value::Array(out);
        }
        ComponentValue::Token(token, span) => (token, span),
    };
    let repr = &source[span.start..span.end];
    match token {
        Token::Ident(value) => json!(["ident", value]),
        Token::AtKeyword(value) => json!(["at-keyword", value]),
        Token::Hash { value, type_flag } => {
            let type_flag = match type_flag {
                HashType::Id => "id",
                HashType::Unrestricted => "unrestricted",
            };
            json!(["hash", value, type_flag])
        }
        Token::String(value) => json!(["string", value]),
        Token::BadString => json!(["error", "bad-string"]),
        Token::Url(value) => json!(["url", value]),
        Token::BadUrl => json!(["error", "bad-url"]),
        Token::Delim(c) => json!(c.to_string()),
        Token::Number(value) => Value::Array(numeric("number", repr, value)),
        Token::Percentage(value) => Value::Array(numeric("percentage", repr, value)),
        Token::Dimension { value, unit } => {
            let mut out = numeric("dimension", repr, value);
            out.push(json!(unit));
            Value::Array(out)
        }
        Token::UnicodeRange { start, end } => json!(["unicode-range", start, end]),
        Token::Whitespace => json!(" "),
        Token::Cdo => json!("<!--"),
        Token::Cdc => json!("-->"),
        Token::Colon => json!(":"),
        Token::Semicolon => json!(";"),
        Token::Comma => json!(","),
        Token::CloseSquare => json!(["error", "]"]),
        Token::CloseParen => json!(["error", ")"]),
        Token::CloseCurly => json!(["error", "}"]),
        token @ (Token::Function(_) | Token::OpenSquare | Token::OpenParen | Token::OpenCurly) => {
            panic!("{token:?} is never a component value")
        }
    }
}

fn component_values_json(source: &str, values: &[ComponentValue]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|value| component_value(source, value))
            .collect(),
    )
}

/// the component values of the {}-block a rule ends with.
fn block_json(source: &str, rule_span: Span) -> Value {
    let text = &source[rule_span.start..rule_span.end];
    match Parser::new().parse_list_of_component_values(text).last() {
        Some(ComponentValue::SimpleBlock(block)) if block.kind == BlockKind::Curly => {
            component_values_json(text, &block.value)
        }
        other => panic!("rule {text:?} doesn't end in a {{}}-block: {other:?}"),
    }
}

fn declaration_json(source: &str, decl: &Declaration) -> Value {
    json!([
        "declaration",
        decl.name,
        component_values_json(source, &decl.value),
        decl.important
    ])
}

fn rule_json(source: &str, rule: &Rule) -> (Span, Value) {
    match rule {
        Rule::At(rule) => {
            let block = match rule.block {
                Some(_) => block_json(source, rule.span),
                None => Value::Null,
            };
            let prelude = component_values_json(source, &rule.prelude);
            (rule.span, json!(["at-rule", rule.name, prelude, block]))
        }
        Rule::Qualified(rule) => {
            let prelude = component_values_json(source, &rule.prelude);
            (
                rule.span,
                json!(["qualified rule", prelude, block_json(source, rule.span)]),
            )
        }
        Rule::NestedDeclarations(_) => panic!("nested declarations at the top level"),
    }
}

/// a list of items, with an `["error", "invalid"]` for each dropped construct among them.
fn list_json(items: Vec<(Span, Value)>, errors: &[ParseError]) -> Value {
    // errors inside an item's span come from inside its blocks.
    let dropped: Vec<usize> = errors
        .iter()
        .filter(|error| error.kind == ParseErrorKind::Invalid)
        .map(|error| error.position)
        .filter(|position| {
            !items
                .iter()
                .any(|(span, _)| span.start <= *position && *position < span.end)
        })
        .collect();
    let mut entries: Vec<(usize, Value)> = items
        .into_iter()
        .map(|(span, value)| (span.start, value))
        .collect();
    entries.extend(
        dropped
            .into_iter()
            .map(|position| (position, json!(["error", "invalid"]))),
    );
    entries.sort_by_key(|(position, _)| *position);
    Value::Array(entries.into_iter().map(|(_, value)| value).collect())
}

fn rules_json(source: &str, rules: &[Rule], errors: &[ParseError]) -> Value {
    list_json(
        rules.iter().map(|rule| rule_json(source, rule)).collect(),
        errors,
    )
}

fn syntax_error(error: SyntaxError) -> Value {
    let kind = match error {
        SyntaxError::Empty => "empty",
        SyntaxError::Invalid => "invalid",
        SyntaxError::ExtraInput => "extra-input",
    };
    json!(["error", kind])
}

fn json_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (
                a.as_f64().unwrap_or(f64::NAN),
                b.as_f64().unwrap_or(f64::NAN),
            );
            (a - b).abs() <= 1e-6 * b.abs().max(1.0)
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| json_eq(a, b))
        }
        _ => a == b,
    }
}

/// removes the suite's eof-in-string and eof-in-url markers, which are parse errors rather than
/// component values, and collects them.
fn strip_eof_errors(value: &Value, found: &mut Vec<ParseErrorKind>) -> Value {
    let Value::Array(items) = value else {
        return value.clone();
    };
    let mut out = Vec::new();
    for item in items {
        if item == &json!(["error", "eof-in-string"]) {
            found.push(ParseErrorKind::EofInString);
        } else if item == &json!(["error", "eof-in-url"]) {
            found.push(ParseErrorKind::EofInUrl);
        } else {
            out.push(strip_eof_errors(item, found));
        }
    }
    Value::Array(out)
}

/// `parse` gets the input and expected result, and returns the actual and expected results to
/// compare.
fn run(file: &str, json: &str, parse: impl Fn(&Value, Value) -> (Value, Value)) {
    let cases: Vec<Value> = serde_json::from_str(json).expect("test file is json");
    let mut failures = Vec::new();
    let mut draft_results = 0;
    for case in cases.chunks(2) {
        let [input, expected] = case else {
            panic!("{file}: odd number of items")
        };
        let key = input
            .as_str()
            .map_or_else(|| input.to_string(), str::to_owned);
        let expected = match DRAFT_RESULTS
            .iter()
            .find(|(f, i, _)| *f == file && *i == key)
        {
            Some((_, _, result)) => {
                draft_results += 1;
                serde_json::from_str(result).expect("draft result is json")
            }
            None => expected.clone(),
        };
        let (actual, expected) = parse(input, expected);
        if !json_eq(&actual, &expected) {
            failures.push(format!(
                "input: {key:?}\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
    }
    let total = cases.len() / 2;
    println!(
        "{file}: {}/{total} pass, {draft_results} against the current draft's result",
        total - failures.len()
    );
    let listed = DRAFT_RESULTS.iter().filter(|(f, _, _)| *f == file).count();
    assert_eq!(
        listed, draft_results,
        "{file}: a DRAFT_RESULTS input isn't in the file"
    );
    assert!(failures.is_empty(), "{file}:\n{}", failures.join("\n"));
}

fn string(input: &Value) -> &str {
    input.as_str().expect("input is a string")
}

macro_rules! suite_file {
    ($name:literal) => {
        include_str!(concat!(
            "../../../../tests/css-parsing-tests/",
            $name,
            ".json"
        ))
    };
}

/// the eof errors among `errors`, once per kind, in the order `strip_eof_errors` reports them.
fn eof_kinds(errors: &[ParseError]) -> Vec<ParseErrorKind> {
    [ParseErrorKind::EofInString, ParseErrorKind::EofInUrl]
        .into_iter()
        .filter(|kind| errors.iter().any(|error| error.kind == *kind))
        .collect()
}

/// component values with the eof errors reported for them, against the expected values with
/// their eof markers moved out.
fn values_with_eof_errors(actual: Value, errors: &[ParseError], expected: Value) -> (Value, Value) {
    let mut found = Vec::new();
    let expected = strip_eof_errors(&expected, &mut found);
    found.sort_by_key(|kind| *kind == ParseErrorKind::EofInUrl);
    found.dedup();
    let debug = |kinds: Vec<ParseErrorKind>| json!(format!("{kinds:?}"));
    (
        json!([actual, debug(eof_kinds(errors))]),
        json!([expected, debug(found)]),
    )
}

#[test]
fn component_value_list() {
    run(
        "component_value_list",
        suite_file!("component_value_list"),
        |input, expected| {
            let source = preprocess(string(input));
            let mut errors = Vec::new();
            // the suite has the tokenizer produce unicode-range tokens everywhere; the current draft
            // only does in the value of a unicode-range descriptor
            // (https://drafts.csswg.org/css-syntax-3/#consume-unicode-range-value), so cases
            // expecting them are run as one.
            let unicode_ranges = expected.to_string().contains("\"unicode-range\"");
            let values = component_values(&source, 0, unicode_ranges, &mut errors);
            let actual = component_values_json(&source, &values);
            values_with_eof_errors(actual, &errors, expected)
        },
    );
}

#[test]
fn one_component_value() {
    run(
        "one_component_value",
        suite_file!("one_component_value"),
        |input, expected| {
            let source = preprocess(string(input));
            let mut parser = Parser::new();
            let actual = match parser.parse_component_value(&source) {
                Ok(value) => component_value(&source, &value),
                Err(error) => syntax_error(error),
            };
            values_with_eof_errors(actual, parser.errors(), expected)
        },
    );
}

#[test]
fn declaration_list() {
    run(
        "declaration_list",
        suite_file!("declaration_list"),
        |input, expected| {
            let source = preprocess(string(input));
            let mut parser = Parser::new();
            let decls = parser.parse_list_of_declarations(&source);
            let items = decls
                .iter()
                .map(|decl| (decl.span, declaration_json(&source, decl)))
                .collect();
            (list_json(items, parser.errors()), expected)
        },
    );
}

#[test]
fn blocks_contents() {
    run(
        "blocks_contents",
        suite_file!("blocks_contents"),
        |input, expected| {
            let source = preprocess(string(input));
            let mut parser = Parser::new();
            let mut items = Vec::new();
            for item in parser.parse_block_contents(&source) {
                match item {
                    BlockItem::Rule(rule) => items.push(rule_json(&source, &rule)),
                    BlockItem::Declarations(decls) => items.extend(
                        decls
                            .iter()
                            .map(|decl| (decl.span, declaration_json(&source, decl))),
                    ),
                }
            }
            (list_json(items, parser.errors()), expected)
        },
    );
}

#[test]
fn one_declaration() {
    run(
        "one_declaration",
        suite_file!("one_declaration"),
        |input, expected| {
            let source = preprocess(string(input));
            let actual = match Parser::new().parse_declaration(&source) {
                Ok(decl) => declaration_json(&source, &decl),
                Err(error) => syntax_error(error),
            };
            (actual, expected)
        },
    );
}

#[test]
fn one_rule() {
    run("one_rule", suite_file!("one_rule"), |input, expected| {
        let source = preprocess(string(input));
        let actual = match Parser::new().parse_rule(&source) {
            Ok(rule) => rule_json(&source, &rule).1,
            Err(error) => syntax_error(error),
        };
        (actual, expected)
    });
}

#[test]
fn rule_list() {
    run("rule_list", suite_file!("rule_list"), |input, expected| {
        let source = preprocess(string(input));
        let mut parser = Parser::new();
        let rules = parser.parse_stylesheet_contents(&source);
        (rules_json(&source, &rules, parser.errors()), expected)
    });
}

#[test]
fn stylesheet() {
    run(
        "stylesheet",
        suite_file!("stylesheet"),
        |input, expected| {
            let source = preprocess(string(input));
            let mut parser = Parser::new();
            let stylesheet = parser.parse_stylesheet(&source);
            (
                rules_json(&source, &stylesheet.rules, parser.errors()),
                expected,
            )
        },
    );
}

#[test]
fn stylesheet_bytes() {
    run(
        "stylesheet_bytes",
        suite_file!("stylesheet_bytes"),
        |input, expected| {
            let label = |key| input.get(key).and_then(Value::as_str);
            let bytes: Vec<u8> = label("css_bytes")
                .expect("css_bytes")
                .chars()
                .map(|c| u8::try_from(c).expect("css_bytes code points are bytes"))
                .collect();
            let environment = label("environment_encoding")
                .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()));
            let (text, encoding) =
                decode_stylesheet(&bytes, label("protocol_encoding"), environment);
            let source = preprocess(&text);
            let mut parser = Parser::new();
            let stylesheet = parser.parse_stylesheet(&source);
            let rules = rules_json(&source, &stylesheet.rules, parser.errors());
            (
                json!([rules, encoding.name().to_ascii_lowercase()]),
                expected,
            )
        },
    );
}

#[test]
fn an_plus_b() {
    run("An+B", suite_file!("An+B"), |input, expected| {
        let values = Parser::new().parse_list_of_component_values(string(input));
        let actual = match parse_a_n_plus_b(&values) {
            Some(AnB { a, b }) => json!([a, b]),
            None => Value::Null,
        };
        (actual, expected)
    });
}
