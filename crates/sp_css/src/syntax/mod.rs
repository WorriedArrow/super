//! css syntax level 3: https://drafts.csswg.org/css-syntax-3/

mod anb;
mod encoding;
mod parser;
mod tokenizer;

#[cfg(test)]
mod css_parsing_tests;

pub use anb::{AnB, parse_a_n_plus_b};
pub use encoding::decode_stylesheet;
pub use parser::Parser;
pub use tokenizer::preprocess;

/// byte offsets into the preprocessed input
/// (https://drafts.csswg.org/css-syntax-3/#input-preprocessing).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// https://drafts.csswg.org/css-syntax-3/#tokenization
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Ident(String),
    Function(String),
    AtKeyword(String),
    Hash {
        value: String,
        type_flag: HashType,
    },
    String(String),
    BadString,
    Url(String),
    BadUrl,
    Delim(char),
    Number(Numeric),
    Percentage(Numeric),
    Dimension {
        value: Numeric,
        unit: String,
    },
    /// only produced when consuming the value of a unicode-range descriptor.
    UnicodeRange {
        start: u32,
        end: u32,
    },
    Whitespace,
    Cdo,
    Cdc,
    Colon,
    Semicolon,
    Comma,
    OpenSquare,
    CloseSquare,
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashType {
    Id,
    Unrestricted,
}

/// the numeric value, type flag and sign character of a <number-token>,
/// <percentage-token> or <dimension-token>.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Numeric {
    pub value: f64,
    pub is_integer: bool,
    pub sign: Option<Sign>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sign {
    Plus,
    Minus,
}

/// https://drafts.csswg.org/css-syntax-3/#component-value
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    /// a preserved token: anything but <function-token>, <{-token>, <[-token> and <(-token>.
    Token(Token, Span),
    Function(Function),
    SimpleBlock(SimpleBlock),
}

impl ComponentValue {
    pub fn span(&self) -> Span {
        match self {
            Self::Token(_, span) => *span,
            Self::Function(function) => function.span,
            Self::SimpleBlock(block) => block.span,
        }
    }
}

/// https://drafts.csswg.org/css-syntax-3/#function
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub name: String,
    pub value: Vec<ComponentValue>,
    pub span: Span,
}

/// https://drafts.csswg.org/css-syntax-3/#simple-block
#[derive(Clone, Debug, PartialEq)]
pub struct SimpleBlock {
    pub kind: BlockKind,
    pub value: Vec<ComponentValue>,
    pub span: Span,
}

/// the associated token of a simple block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    Curly,
    Square,
    Paren,
}

/// https://drafts.csswg.org/css-syntax-3/#css-stylesheet
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

/// https://drafts.csswg.org/css-syntax-3/#css-rule
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    At(AtRule),
    Qualified(QualifiedRule),
    /// a list of declarations that followed a child rule in a qualified rule's block
    /// (https://drafts.csswg.org/css-syntax-3/#consume-qualified-rule).
    NestedDeclarations(Vec<Declaration>),
}

/// https://drafts.csswg.org/css-syntax-3/#at-rule
#[derive(Clone, Debug, PartialEq)]
pub struct AtRule {
    pub name: String,
    pub prelude: Vec<ComponentValue>,
    /// the result of consuming the rule's {}-block, none for statement at-rules. how the
    /// declaration lists in it materialize depends on the rule.
    pub block: Option<Vec<BlockItem>>,
    pub span: Span,
}

/// https://drafts.csswg.org/css-syntax-3/#qualified-rule
#[derive(Clone, Debug, PartialEq)]
pub struct QualifiedRule {
    pub prelude: Vec<ComponentValue>,
    pub declarations: Vec<Declaration>,
    pub child_rules: Vec<Rule>,
    pub span: Span,
}

/// an item of the list returned by https://drafts.csswg.org/css-syntax-3/#consume-block-contents
#[derive(Clone, Debug, PartialEq)]
pub enum BlockItem {
    Rule(Rule),
    Declarations(Vec<Declaration>),
}

/// https://drafts.csswg.org/css-syntax-3/#declaration
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    pub name: String,
    pub value: Vec<ComponentValue>,
    pub important: bool,
    /// captured for custom properties only.
    pub original_text: Option<String>,
    pub span: Span,
}

/// the syntax error an entry point returns
/// (https://drafts.csswg.org/css-syntax-3/#parser-entry-points).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxError {
    /// the input held nothing but whitespace and comments.
    Empty,
    /// the construct couldn't be consumed.
    Invalid,
    /// something other than whitespace followed the construct.
    ExtraInput,
}

/// a parse error, at a byte offset into the preprocessed input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub kind: ParseErrorKind,
    pub position: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    EofInComment,
    EofInString,
    NewlineInString,
    EofInUrl,
    BadUrl,
    EofInEscape,
    InvalidEscape,
    /// a rule or declaration was dropped from a list.
    Invalid,
}
