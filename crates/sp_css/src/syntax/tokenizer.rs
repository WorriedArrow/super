//! https://drafts.csswg.org/css-syntax-3/#tokenization

use std::borrow::Cow;

use super::{HashType, Numeric, ParseError, ParseErrorKind, Sign, Span, Token};

/// https://drafts.csswg.org/css-syntax-3/#input-preprocessing
pub fn preprocess(input: &str) -> Cow<'_, str> {
    if !input.contains(['\r', '\u{c}', '\0']) {
        return Cow::Borrowed(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // step 1: cr, ff and cr lf become lf.
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{c}' => out.push('\n'),
            // step 2: null becomes u+fffd. a rust string can't hold surrogates.
            '\0' => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// https://drafts.csswg.org/css-syntax-3/#whitespace
fn is_whitespace(c: Option<char>) -> bool {
    matches!(c, Some('\n' | '\t' | ' '))
}

fn is_digit(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_digit())
}

fn is_hex_digit(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_hexdigit())
}

/// https://drafts.csswg.org/css-syntax-3/#non-ascii-ident-code-point
fn is_non_ascii_ident_code_point(c: char) -> bool {
    matches!(c as u32,
        0xb7 | 0xc0..=0xd6 | 0xd8..=0xf6 | 0xf8..=0x37d | 0x37f..=0x1fff | 0x200c | 0x200d
        | 0x203f | 0x2040 | 0x2070..=0x218f | 0x2c00..=0x2fef | 0x3001..=0xd7ff
        | 0xf900..=0xfdcf | 0xfdf0..=0xfffd | 0x10000..)
}

/// https://drafts.csswg.org/css-syntax-3/#ident-start-code-point
fn is_ident_start(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || is_non_ascii_ident_code_point(c))
}

/// https://drafts.csswg.org/css-syntax-3/#ident-code-point
fn is_ident_code_point(c: Option<char>) -> bool {
    is_ident_start(c) || is_digit(c) || c == Some('-')
}

/// https://drafts.csswg.org/css-syntax-3/#non-printable-code-point
fn is_non_printable(c: char) -> bool {
    matches!(c, '\0'..='\u{8}' | '\u{b}' | '\u{e}'..='\u{1f}' | '\u{7f}')
}

/// https://drafts.csswg.org/css-syntax-3/#starts-with-a-valid-escape
fn is_valid_escape(first: Option<char>, second: Option<char>) -> bool {
    first == Some('\\') && second != Some('\n')
}

/// https://drafts.csswg.org/css-syntax-3/#would-start-an-identifier
fn would_start_ident(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    match first {
        Some('-') => {
            is_ident_start(second) || second == Some('-') || is_valid_escape(second, third)
        }
        Some('\\') => is_valid_escape(first, second),
        _ => is_ident_start(first),
    }
}

/// https://drafts.csswg.org/css-syntax-3/#starts-with-a-number
fn would_start_number(first: Option<char>, second: Option<char>, third: Option<char>) -> bool {
    match first {
        Some('+' | '-') => is_digit(second) || (second == Some('.') && is_digit(third)),
        Some('.') => is_digit(second),
        _ => is_digit(first),
    }
}

/// https://drafts.csswg.org/css-syntax-3/#starts-a-unicode-range
fn would_start_unicode_range(
    first: Option<char>,
    second: Option<char>,
    third: Option<char>,
) -> bool {
    matches!(first, Some('u' | 'U'))
        && second == Some('+')
        && (third == Some('?') || is_hex_digit(third))
}

/// tokenizes preprocessed input on demand. spans and errors are offset by `offset`, so a
/// segment of a larger input can be retokenized in place.
pub(crate) struct Tokenizer<'a, 'e> {
    input: &'a str,
    pos: usize,
    offset: usize,
    unicode_ranges_allowed: bool,
    errors: &'e mut Vec<ParseError>,
}

impl<'a, 'e> Tokenizer<'a, 'e> {
    pub(crate) fn new(
        input: &'a str,
        offset: usize,
        unicode_ranges_allowed: bool,
        errors: &'e mut Vec<ParseError>,
    ) -> Self {
        Self {
            input,
            pos: 0,
            offset,
            unicode_ranges_allowed,
            errors,
        }
    }

    /// the end of the input, offset like the spans.
    pub(crate) fn end(&self) -> usize {
        self.offset + self.input.len()
    }

    /// the code point `n` places after the next input code point; `peek(0)` is the next one.
    fn peek(&self, n: usize) -> Option<char> {
        self.input[self.pos..].chars().nth(n)
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek(0)?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn error(&mut self, kind: ParseErrorKind) {
        self.errors.push(ParseError {
            kind,
            position: self.offset + self.pos,
        });
    }

    fn consume_whitespace(&mut self) {
        while is_whitespace(self.peek(0)) {
            self.advance();
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-token
    /// returns none at eof.
    pub(crate) fn next_token(&mut self) -> Option<(Token, Span)> {
        self.consume_comments();
        let start = self.pos;
        let (c, p1, p2, p3) = (self.peek(0)?, self.peek(1), self.peek(2), self.peek(3));
        let token = match c {
            '\n' | '\t' | ' ' => {
                self.consume_whitespace();
                Token::Whitespace
            }
            '"' | '\'' => {
                self.advance();
                self.consume_string_token(c)
            }
            '#' if is_ident_code_point(p1) || is_valid_escape(p1, p2) => {
                let type_flag = if would_start_ident(p1, p2, p3) {
                    HashType::Id
                } else {
                    HashType::Unrestricted
                };
                self.advance();
                Token::Hash {
                    value: self.consume_ident_sequence(),
                    type_flag,
                }
            }
            '+' | '-' | '.' if would_start_number(Some(c), p1, p2) => self.consume_numeric_token(),
            '-' if p1 == Some('-') && p2 == Some('>') => {
                self.pos += 3;
                Token::Cdc
            }
            '-' if would_start_ident(Some(c), p1, p2) => self.consume_ident_like_token(),
            '<' if (p1, p2, p3) == (Some('!'), Some('-'), Some('-')) => {
                self.pos += 4;
                Token::Cdo
            }
            '@' if would_start_ident(p1, p2, p3) => {
                self.advance();
                Token::AtKeyword(self.consume_ident_sequence())
            }
            '\\' if is_valid_escape(Some(c), p1) => self.consume_ident_like_token(),
            '\\' => {
                self.error(ParseErrorKind::InvalidEscape);
                self.advance();
                Token::Delim(c)
            }
            '0'..='9' => self.consume_numeric_token(),
            'u' | 'U'
                if self.unicode_ranges_allowed && would_start_unicode_range(Some(c), p1, p2) =>
            {
                self.consume_unicode_range_token()
            }
            _ if is_ident_start(Some(c)) => self.consume_ident_like_token(),
            _ => {
                self.advance();
                match c {
                    '(' => Token::OpenParen,
                    ')' => Token::CloseParen,
                    ',' => Token::Comma,
                    ':' => Token::Colon,
                    ';' => Token::Semicolon,
                    '[' => Token::OpenSquare,
                    ']' => Token::CloseSquare,
                    '{' => Token::OpenCurly,
                    '}' => Token::CloseCurly,
                    _ => Token::Delim(c),
                }
            }
        };
        let span = Span {
            start: self.offset + start,
            end: self.offset + self.pos,
        };
        Some((token, span))
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-comment
    fn consume_comments(&mut self) {
        while self.input[self.pos..].starts_with("/*") {
            match self.input[self.pos + 2..].find("*/") {
                Some(end) => self.pos += 2 + end + 2,
                None => {
                    self.pos = self.input.len();
                    self.error(ParseErrorKind::EofInComment);
                }
            }
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-numeric-token
    fn consume_numeric_token(&mut self) -> Token {
        let value = self.consume_number();
        if would_start_ident(self.peek(0), self.peek(1), self.peek(2)) {
            let unit = self.consume_ident_sequence();
            Token::Dimension { value, unit }
        } else if self.peek(0) == Some('%') {
            self.advance();
            Token::Percentage(value)
        } else {
            Token::Number(value)
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-ident-like-token
    fn consume_ident_like_token(&mut self) -> Token {
        let string = self.consume_ident_sequence();
        if self.peek(0) != Some('(') {
            return Token::Ident(string);
        }
        self.advance();
        if string.eq_ignore_ascii_case("url") {
            while is_whitespace(self.peek(0)) && is_whitespace(self.peek(1)) {
                self.advance();
            }
            let (first, second) = (self.peek(0), self.peek(1));
            let is_quote = |c| matches!(c, Some('"' | '\''));
            if is_quote(first) || (is_whitespace(first) && is_quote(second)) {
                return Token::Function(string);
            }
            return self.consume_url_token();
        }
        Token::Function(string)
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-string-token
    fn consume_string_token(&mut self, ending: char) -> Token {
        let mut value = String::new();
        loop {
            match self.advance() {
                Some(c) if c == ending => return Token::String(value),
                None => {
                    self.error(ParseErrorKind::EofInString);
                    return Token::String(value);
                }
                Some('\n') => {
                    self.pos -= 1;
                    self.error(ParseErrorKind::NewlineInString);
                    return Token::BadString;
                }
                Some('\\') => match self.peek(0) {
                    None => {}
                    Some('\n') => {
                        self.advance();
                    }
                    Some(_) => value.push(self.consume_escaped_code_point()),
                },
                Some(c) => value.push(c),
            }
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-url-token
    fn consume_url_token(&mut self) -> Token {
        let mut value = String::new();
        self.consume_whitespace();
        loop {
            match self.advance() {
                Some(')') => return Token::Url(value),
                None => {
                    self.error(ParseErrorKind::EofInUrl);
                    return Token::Url(value);
                }
                Some('\n' | '\t' | ' ') => {
                    self.consume_whitespace();
                    match self.peek(0) {
                        Some(')') => {
                            self.advance();
                            return Token::Url(value);
                        }
                        None => {
                            self.error(ParseErrorKind::EofInUrl);
                            return Token::Url(value);
                        }
                        Some(_) => {
                            self.consume_remnants_of_bad_url();
                            return Token::BadUrl;
                        }
                    }
                }
                Some('\\') if is_valid_escape(Some('\\'), self.peek(0)) => {
                    value.push(self.consume_escaped_code_point());
                }
                Some(c) if matches!(c, '"' | '\'' | '(' | '\\') || is_non_printable(c) => {
                    self.error(ParseErrorKind::BadUrl);
                    self.consume_remnants_of_bad_url();
                    return Token::BadUrl;
                }
                Some(c) => value.push(c),
            }
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-escaped-code-point
    /// the reverse solidus has been consumed already.
    fn consume_escaped_code_point(&mut self) -> char {
        match self.advance() {
            Some(c) if c.is_ascii_hexdigit() => {
                let mut value = c.to_digit(16).unwrap_or(0);
                for _ in 0..5 {
                    match self.peek(0).and_then(|c| c.to_digit(16)) {
                        Some(digit) => {
                            value = value * 16 + digit;
                            self.advance();
                        }
                        None => break,
                    }
                }
                if is_whitespace(self.peek(0)) {
                    self.advance();
                }
                // zero, surrogates and anything past u+10ffff become u+fffd; from_u32 refuses the
                // last two.
                match value {
                    0 => '\u{fffd}',
                    _ => char::from_u32(value).unwrap_or('\u{fffd}'),
                }
            }
            None => {
                self.error(ParseErrorKind::EofInEscape);
                '\u{fffd}'
            }
            Some(c) => c,
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-name
    fn consume_ident_sequence(&mut self) -> String {
        let mut result = String::new();
        loop {
            let c = self.peek(0);
            if is_ident_code_point(c) {
                result.extend(self.advance());
            } else if is_valid_escape(c, self.peek(1)) {
                self.advance();
                result.push(self.consume_escaped_code_point());
            } else {
                return result;
            }
        }
    }

    fn consume_digits(&mut self) {
        while is_digit(self.peek(0)) {
            self.advance();
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-number
    fn consume_number(&mut self) -> Numeric {
        let input = self.input;
        let start = self.pos;
        // step 1
        let mut is_integer = true;
        // step 2
        let sign = match self.peek(0) {
            Some('+') => Some(Sign::Plus),
            Some('-') => Some(Sign::Minus),
            _ => None,
        };
        if sign.is_some() {
            self.advance();
        }
        // step 3
        self.consume_digits();
        // step 4
        if self.peek(0) == Some('.') && is_digit(self.peek(1)) {
            self.advance();
            self.consume_digits();
            is_integer = false;
        }
        let number_part = &input[start..self.pos];
        // step 5
        let mut exponent_part = "";
        let (p0, p1, p2) = (self.peek(0), self.peek(1), self.peek(2));
        if matches!(p0, Some('e' | 'E'))
            && (is_digit(p1) || (matches!(p1, Some('+' | '-')) && is_digit(p2)))
        {
            self.advance();
            let exponent_start = self.pos;
            if matches!(p1, Some('+' | '-')) {
                self.advance();
            }
            self.consume_digits();
            exponent_part = &input[exponent_start..self.pos];
            is_integer = false;
        }
        // step 6: parsing the decimal string as a whole gives the correctly rounded value of
        // number part times ten to the exponent part.
        let value = if exponent_part.is_empty() {
            number_part.parse::<f64>()
        } else {
            format!("{number_part}e{exponent_part}").parse::<f64>()
        };
        Numeric {
            value: value.unwrap_or(0.0),
            is_integer,
            sign,
        }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-unicode-range-token
    fn consume_unicode_range_token(&mut self) -> Token {
        let input = self.input;
        // step 1
        self.pos += 2;
        // step 2
        let first_start = self.pos;
        let mut count = 0;
        while count < 6 && is_hex_digit(self.peek(0)) {
            self.advance();
            count += 1;
        }
        let mut question_marks = 0;
        while count < 6 && self.peek(0) == Some('?') {
            self.advance();
            count += 1;
            question_marks += 1;
        }
        let first_segment = &input[first_start..self.pos];
        let hex = |digits: &str| u32::from_str_radix(digits, 16).unwrap_or(0);
        // step 3
        if question_marks > 0 {
            return Token::UnicodeRange {
                start: hex(&first_segment.replace('?', "0")),
                end: hex(&first_segment.replace('?', "F")),
            };
        }
        // step 4
        let start = hex(first_segment);
        // step 5
        if self.peek(0) == Some('-') && is_hex_digit(self.peek(1)) {
            self.advance();
            let end_start = self.pos;
            let mut count = 0;
            while count < 6 && is_hex_digit(self.peek(0)) {
                self.advance();
                count += 1;
            }
            return Token::UnicodeRange {
                start,
                end: hex(&input[end_start..self.pos]),
            };
        }
        // step 6
        Token::UnicodeRange { start, end: start }
    }

    /// https://drafts.csswg.org/css-syntax-3/#consume-remnants-of-bad-url
    fn consume_remnants_of_bad_url(&mut self) {
        loop {
            match self.advance() {
                None | Some(')') => return,
                Some('\\') if is_valid_escape(Some('\\'), self.peek(0)) => {
                    self.consume_escaped_code_point();
                }
                Some(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        let mut errors = Vec::new();
        let mut tokenizer = Tokenizer::new(input, 0, false, &mut errors);
        std::iter::from_fn(|| tokenizer.next_token().map(|(token, _)| token)).collect()
    }

    fn number(value: f64, is_integer: bool, sign: Option<Sign>) -> Numeric {
        Numeric {
            value,
            is_integer,
            sign,
        }
    }

    #[test]
    fn preprocessing_normalizes_newlines_and_nulls() {
        assert_eq!(preprocess("a\r\nb\rc\u{c}d\0"), "a\nb\nc\nd\u{fffd}");
        assert!(matches!(preprocess("plain"), Cow::Borrowed("plain")));
    }

    #[test]
    fn numbers_keep_sign_and_type() {
        assert_eq!(
            tokens("+.5 -3 1e3"),
            [
                Token::Number(number(0.5, false, Some(Sign::Plus))),
                Token::Whitespace,
                Token::Number(number(-3.0, true, Some(Sign::Minus))),
                Token::Whitespace,
                Token::Number(number(1000.0, false, None)),
            ]
        );
        assert_eq!(
            tokens("1e999999999999"),
            [Token::Number(number(f64::INFINITY, false, None))]
        );
    }

    #[test]
    fn unicode_ranges_only_when_allowed() {
        assert_eq!(
            tokens("u+1F"),
            [
                Token::Ident("u".into()),
                Token::Dimension {
                    value: number(1.0, true, Some(Sign::Plus)),
                    unit: "F".into()
                }
            ]
        );
        let mut errors = Vec::new();
        let mut tokenizer = Tokenizer::new("u+1F-2a", 0, true, &mut errors);
        assert_eq!(
            tokenizer.next_token(),
            Some((
                Token::UnicodeRange {
                    start: 0x1f,
                    end: 0x2a
                },
                Span { start: 0, end: 7 }
            ))
        );
    }

    #[test]
    fn escapes_out_of_range_become_replacement() {
        assert_eq!(
            tokens("\\d800 \\110000 \\0"),
            [Token::Ident("\u{fffd}\u{fffd}\u{fffd}".into()),]
        );
    }

    #[test]
    fn errors_are_reported_where_they_happen() {
        let mut errors = Vec::new();
        let mut tokenizer = Tokenizer::new("a /* x", 10, false, &mut errors);
        while tokenizer.next_token().is_some() {}
        assert_eq!(
            errors,
            [ParseError {
                kind: ParseErrorKind::EofInComment,
                position: 16
            }]
        );
    }
}
