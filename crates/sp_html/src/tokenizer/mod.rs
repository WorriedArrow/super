// https://html.spec.whatwg.org/multipage/parsing.html#tokenization

mod entities;
mod input;
mod token;

use std::collections::{HashSet, VecDeque};
use std::mem;

pub use token::{Attribute, Doctype, ProcessingInstruction, Tag, Token};

use crate::Wtf8Buf;
use crate::error::{ErrorCode, ParseError};
use entities::NameMatcher;
use input::{InputStream, Next, Peeked};

/// The tokenizer states, named as in the spec's subsections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThanSign,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    ProcessingInstructionOpen,
    ProcessingInstructionTarget,
    AfterProcessingInstructionTarget,
    ProcessingInstructionData,
    ProcessingInstructionQuestionable,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

/// An input character: a scalar value, a lone surrogate, or EOF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ch {
    C(char),
    Surrogate(u32),
    Eof,
}

use Ch::{C, Eof};

impl Ch {
    fn from_code_point(cp: u32) -> Ch {
        char::from_u32(cp).map_or(Ch::Surrogate(cp), C)
    }

    fn is_whitespace(self) -> bool {
        matches!(self, C('\t' | '\n' | '\x0C' | ' '))
    }

    fn is_ascii_alpha(self) -> bool {
        matches!(self, C(c) if c.is_ascii_alphabetic())
    }

    fn is_ascii_alphanumeric(self) -> bool {
        matches!(self, C(c) if c.is_ascii_alphanumeric())
    }
}

fn push_ch(buf: &mut Wtf8Buf, c: Ch) {
    match c {
        C(c) => buf.push(c),
        Ch::Surrogate(cp) => buf.push_code_point(cp),
        Eof => debug_assert!(false, "EOF is never appended"),
    }
}

enum Step {
    Continue,
    /// out of input (or at the insertion point); the state is left so that running it
    /// again with more input carries on where it stopped.
    Suspend,
}

enum Lookahead {
    Match,
    NoMatch,
    NeedMoreInput,
}

macro_rules! consume {
    ($self:ident) => {
        match $self.consume() {
            Some(c) => c,
            None => return Step::Suspend,
        }
    };
}

/// The html tokenizer. Feed it input with [`Tokenizer::push_str`] and pull tokens with
/// [`Tokenizer::next_token`]; it returns `None` when it needs more input. The tree
/// builder handles each token before asking for the next, so it can switch the state
/// in between, as the spec's "emit" requires.
pub struct Tokenizer {
    input: InputStream,
    state: State,
    return_state: State,
    reconsume: bool,
    current: Ch,
    stop_at_insertion_point: bool,
    temp: String,
    char_ref_code: u32,
    tag: Tag,
    tag_is_end: bool,
    attr_name: Wtf8Buf,
    attr_value: Wtf8Buf,
    attr_open: bool,
    attr_removed: bool,
    attr_index: HashSet<Wtf8Buf>,
    comment: Wtf8Buf,
    doctype: Doctype,
    pi: ProcessingInstruction,
    last_start_tag: Option<Wtf8Buf>,
    chars: Wtf8Buf,
    queue: VecDeque<Token>,
    errors: Option<Vec<ParseError>>,
    cdata_allowed: bool,
    eof_emitted: bool,
}

impl Default for Tokenizer {
    fn default() -> Self {
        Tokenizer::new()
    }
}

/// Above this many attributes on one tag, the duplicate check uses a hash set instead
/// of scanning, so a tag with a huge attribute list isn't quadratic.
const ATTR_SCAN_LIMIT: usize = 16;

impl Tokenizer {
    pub fn new() -> Self {
        Tokenizer {
            input: InputStream::new(),
            state: State::Data,
            return_state: State::Data,
            reconsume: false,
            current: Eof,
            stop_at_insertion_point: false,
            temp: String::new(),
            char_ref_code: 0,
            tag: Tag::default(),
            tag_is_end: false,
            attr_name: Wtf8Buf::new(),
            attr_value: Wtf8Buf::new(),
            attr_open: false,
            attr_removed: false,
            attr_index: HashSet::new(),
            comment: Wtf8Buf::new(),
            doctype: Doctype::default(),
            pi: ProcessingInstruction::default(),
            last_start_tag: None,
            chars: Wtf8Buf::new(),
            queue: VecDeque::new(),
            errors: None,
            cdata_allowed: false,
            eof_emitted: false,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// Switches the state, as the tree construction stage does for rcdata, rawtext,
    /// script data and plaintext elements.
    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    /// Sets "the tag name of the last start tag to have been emitted from this
    /// tokenizer", used for appropriate end tags. Emitting a start tag updates it too.
    pub fn set_last_start_tag(&mut self, name: Option<&str>) {
        self.last_start_tag = name.map(Wtf8Buf::from);
    }

    /// Whether there is an adjusted current node that is not an element in the html
    /// namespace, which the markup declaration open state checks for `<![CDATA[`.
    pub fn set_cdata_allowed(&mut self, allowed: bool) {
        self.cdata_allowed = allowed;
    }

    /// Starts or stops recording parse errors. Off by default.
    pub fn set_report_errors(&mut self, on: bool) {
        self.errors = on.then(Vec::new);
    }

    /// Takes the parse errors recorded so far.
    pub fn take_errors(&mut self) -> Vec<ParseError> {
        self.errors.as_mut().map(mem::take).unwrap_or_default()
    }

    /// Appends decoded input to the end of the input stream.
    pub fn push_str(&mut self, text: &str) {
        self.input.append(text.into());
    }

    /// Appends input that may contain lone surrogates.
    pub fn push_wtf8(&mut self, text: Wtf8Buf) {
        self.input.append(text);
    }

    /// Marks the end of the input stream: once everything before it is consumed, the
    /// tokenizer sees EOF.
    pub fn close(&mut self) {
        self.input.close();
    }

    /// https://html.spec.whatwg.org/multipage/parsing.html#insertion-point
    /// Sets the insertion point just before the next input character.
    pub fn set_insertion_point(&mut self) {
        debug_assert!(!self.reconsume, "insertion point set between tokens");
        self.input.set_insertion_point_before_next();
    }

    /// Sets the insertion point just before the end of the input stream.
    pub fn set_insertion_point_at_end(&mut self) {
        self.input.set_insertion_point_at_end();
    }

    pub fn clear_insertion_point(&mut self) {
        self.input.clear_insertion_point();
    }

    pub fn has_insertion_point(&self) -> bool {
        self.input.has_insertion_point()
    }

    /// Inserts text just before the insertion point, as `document.write()` does. With
    /// no insertion point, appends to the end.
    pub fn insert_at_insertion_point(&mut self, text: Wtf8Buf) {
        self.input.insert(text);
    }

    /// The next token, or `None` when all available input is consumed (or after the
    /// end-of-file token).
    pub fn next_token(&mut self) -> Option<Token> {
        self.run(false)
    }

    /// Like [`Tokenizer::next_token`], but treats the insertion point as the end of the
    /// available input, for `document.write()`'s "process input until the tokenizer
    /// reaches the insertion point".
    pub fn next_token_before_insertion_point(&mut self) -> Option<Token> {
        self.run(true)
    }

    fn run(&mut self, stop_at_insertion_point: bool) -> Option<Token> {
        self.stop_at_insertion_point = stop_at_insertion_point;
        loop {
            if let Some(token) = self.queue.pop_front() {
                return Some(token);
            }
            if self.eof_emitted {
                return None;
            }
            if let Step::Suspend = self.step() {
                if self.chars.is_empty() {
                    return None;
                }
                return Some(Token::Characters(mem::take(&mut self.chars)));
            }
        }
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#preprocessing-the-input-stream
    fn consume(&mut self) -> Option<Ch> {
        if self.reconsume {
            self.reconsume = false;
            return Some(self.current);
        }
        let c = match self.input.consume(self.stop_at_insertion_point) {
            Next::Suspend => return None,
            Next::Eof => Eof,
            Next::Char(cp) => {
                if self.errors.is_some() {
                    if (0xD800..=0xDFFF).contains(&cp) {
                        self.error(ErrorCode::SurrogateInInputStream);
                    } else if is_noncharacter(cp) {
                        self.error(ErrorCode::NoncharacterInInputStream);
                    } else if is_control(cp) && !matches!(cp, 0x00 | 0x09 | 0x0A | 0x0C | 0x0D) {
                        self.error(ErrorCode::ControlCharacterInInputStream);
                    }
                }
                Ch::from_code_point(cp)
            }
        };
        self.current = c;
        Some(c)
    }

    /// "Reconsume in the X state".
    fn reconsume_in(&mut self, state: State) {
        self.reconsume = true;
        self.state = state;
    }

    /// The upcoming characters, starting with the current one if it's to be reconsumed.
    fn peek(&self) -> impl Iterator<Item = Peeked> + '_ {
        let current = self.reconsume.then_some(match self.current {
            C(c) => Peeked::Char(c.into()),
            Ch::Surrogate(cp) => Peeked::Char(cp),
            Eof => Peeked::Eof,
        });
        current
            .into_iter()
            .chain(self.input.peek(self.stop_at_insertion_point))
    }

    /// Whether the next characters are `s`, optionally ASCII case-insensitively.
    fn lookahead(&self, s: &str, case_insensitive: bool) -> Lookahead {
        let mut next = self.peek();
        for expected in s.chars() {
            match next.next() {
                Some(Peeked::Char(cp)) => {
                    let matched = char::from_u32(cp).is_some_and(|c| {
                        if case_insensitive {
                            c.eq_ignore_ascii_case(&expected)
                        } else {
                            c == expected
                        }
                    });
                    if !matched {
                        return Lookahead::NoMatch;
                    }
                }
                Some(Peeked::End) => return Lookahead::NeedMoreInput,
                Some(Peeked::Eof) | None => return Lookahead::NoMatch,
            }
        }
        Lookahead::Match
    }

    /// Consumes `n` characters that a lookahead already matched.
    fn consume_matched(&mut self, n: usize) {
        for _ in 0..n {
            let c = self.consume();
            debug_assert!(matches!(c, Some(C(_))));
        }
    }

    fn error(&mut self, code: ErrorCode) {
        let (line, col) = self.input.position();
        self.error_at(code, line, col);
    }

    /// Reports an error at the next input character, for the character reference
    /// errors that are detected once the reference has been consumed.
    fn error_at_next_char(&mut self, code: ErrorCode) {
        let (line, col) = if self.reconsume {
            self.input.position()
        } else {
            self.input.next_position()
        };
        self.error_at(code, line, col);
    }

    fn error_at(&mut self, code: ErrorCode, line: u32, col: u32) {
        if let Some(errors) = &mut self.errors {
            errors.push(ParseError { code, line, col });
        }
    }

    fn emit_ch(&mut self, c: Ch) {
        push_ch(&mut self.chars, c);
    }

    fn emit_char(&mut self, c: char) {
        self.chars.push(c);
    }

    fn emit(&mut self, token: Token) {
        if !self.chars.is_empty() {
            self.queue
                .push_back(Token::Characters(mem::take(&mut self.chars)));
        }
        self.queue.push_back(token);
    }

    fn emit_eof(&mut self) {
        self.emit(Token::Eof);
        self.eof_emitted = true;
    }

    fn emit_comment(&mut self) {
        let data = mem::take(&mut self.comment);
        self.emit(Token::Comment(data));
    }

    fn emit_doctype(&mut self) {
        let doctype = mem::take(&mut self.doctype);
        self.emit(Token::Doctype(doctype));
    }

    fn new_tag(&mut self, is_end: bool) {
        self.tag = Tag::default();
        self.tag_is_end = is_end;
        self.attr_open = false;
        self.attr_index.clear();
    }

    fn emit_tag(&mut self) {
        self.finish_attribute();
        let tag = mem::take(&mut self.tag);
        if self.tag_is_end {
            if !tag.attributes.is_empty() {
                self.error(ErrorCode::EndTagWithAttributes);
            }
            if tag.self_closing {
                self.error(ErrorCode::EndTagWithTrailingSolidus);
            }
            self.emit(Token::EndTag(tag));
        } else {
            self.last_start_tag = Some(tag.name.clone());
            self.emit(Token::StartTag(tag));
        }
    }

    /// "Start a new attribute in the current tag token."
    fn start_attribute(&mut self) {
        self.finish_attribute();
        self.attr_name.clear();
        self.attr_value.clear();
        self.attr_open = true;
        self.attr_removed = false;
    }

    fn finish_attribute(&mut self) {
        if mem::take(&mut self.attr_open) && !self.attr_removed {
            self.tag.attributes.push(Attribute {
                name: mem::take(&mut self.attr_name),
                value: mem::take(&mut self.attr_value),
            });
        }
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#attribute-name-state
    // "when the user agent leaves the attribute name state ... if there is already an
    // attribute on the token with the exact same name, then this is a
    // duplicate-attribute parse error and the new attribute must be removed".
    fn leave_attribute_name(&mut self) {
        let attributes = &self.tag.attributes;
        let duplicate = if attributes.len() <= ATTR_SCAN_LIMIT {
            attributes.iter().any(|a| a.name == self.attr_name)
        } else {
            for a in &attributes[self.attr_index.len()..] {
                self.attr_index.insert(a.name.clone());
            }
            self.attr_index.contains(&self.attr_name)
        };
        if duplicate {
            self.error(ErrorCode::DuplicateAttribute);
            self.attr_removed = true;
        }
    }

    fn is_appropriate_end_tag(&self) -> bool {
        self.last_start_tag.as_ref() == Some(&self.tag.name)
    }

    fn char_ref_in_attribute(&self) -> bool {
        matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }

    /// "Flush code points consumed as a character reference."
    fn flush_char_ref(&mut self) {
        if self.char_ref_in_attribute() {
            self.attr_value.push_str(&self.temp);
        } else {
            self.chars.push_str(&self.temp);
        }
    }

    /// "Convert the temporary buffer to a comment."
    fn temp_to_comment(&mut self) {
        self.comment.clear();
        self.comment.push('?');
        self.comment.push_str(&self.temp);
    }

    /// The shared "anything else" of the rcdata, rawtext and script data end tag name
    /// states: emit "</" and the temporary buffer, and reconsume in `state`.
    fn end_tag_name_anything_else(&mut self, state: State) {
        self.chars.push_str("</");
        self.chars.push_str(&self.temp);
        self.reconsume_in(state);
    }

    /// RCDATA, RAWTEXT, script data and script data escaped end tag name states.
    fn end_tag_name_state(&mut self, c: Ch, fallback: State) {
        match c {
            c if c.is_whitespace() && self.is_appropriate_end_tag() => {
                self.state = State::BeforeAttributeName;
            }
            C('/') if self.is_appropriate_end_tag() => self.state = State::SelfClosingStartTag,
            C('>') if self.is_appropriate_end_tag() => {
                self.state = State::Data;
                self.emit_tag();
            }
            C(c) if c.is_ascii_alphabetic() => {
                self.tag.name.push(c.to_ascii_lowercase());
                self.temp.push(c);
            }
            _ => self.end_tag_name_anything_else(fallback),
        }
    }

    /// Runs one step of the state machine: usually one input character.
    fn step(&mut self) -> Step {
        match self.state {
            // https://html.spec.whatwg.org/multipage/parsing.html#data-state
            State::Data => match consume!(self) {
                C('&') => {
                    self.return_state = State::Data;
                    self.state = State::CharacterReference;
                }
                C('<') => self.state = State::TagOpen,
                c @ C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_ch(c);
                }
                Eof => self.emit_eof(),
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rcdata-state
            State::Rcdata => match consume!(self) {
                C('&') => {
                    self.return_state = State::Rcdata;
                    self.state = State::CharacterReference;
                }
                C('<') => self.state = State::RcdataLessThanSign,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => self.emit_eof(),
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rawtext-state
            State::Rawtext => match consume!(self) {
                C('<') => self.state = State::RawtextLessThanSign,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => self.emit_eof(),
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-state
            State::ScriptData => match consume!(self) {
                C('<') => self.state = State::ScriptDataLessThanSign,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => self.emit_eof(),
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#plaintext-state
            State::Plaintext => match consume!(self) {
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => self.emit_eof(),
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#tag-open-state
            State::TagOpen => match consume!(self) {
                C('!') => self.state = State::MarkupDeclarationOpen,
                C('/') => self.state = State::EndTagOpen,
                c if c.is_ascii_alpha() => {
                    self.new_tag(false);
                    self.reconsume_in(State::TagName);
                }
                C('?') => {
                    self.temp.clear();
                    self.state = State::ProcessingInstructionOpen;
                }
                Eof => {
                    self.error(ErrorCode::EofBeforeTagName);
                    self.emit_char('<');
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::InvalidFirstCharacterOfTagName);
                    self.emit_char('<');
                    self.reconsume_in(State::Data);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#end-tag-open-state
            State::EndTagOpen => match consume!(self) {
                c if c.is_ascii_alpha() => {
                    self.new_tag(true);
                    self.reconsume_in(State::TagName);
                }
                C('>') => {
                    self.error(ErrorCode::MissingEndTagName);
                    self.state = State::Data;
                }
                Eof => {
                    self.error(ErrorCode::EofBeforeTagName);
                    self.chars.push_str("</");
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::InvalidFirstCharacterOfTagName);
                    self.comment.clear();
                    self.reconsume_in(State::BogusComment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#tag-name-state
            State::TagName => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeAttributeName,
                C('/') => self.state = State::SelfClosingStartTag,
                C('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                C(c) if c.is_ascii_uppercase() => self.tag.name.push(c.to_ascii_lowercase()),
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.tag.name.push('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                c => push_ch(&mut self.tag.name, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rcdata-less-than-sign-state
            State::RcdataLessThanSign => match consume!(self) {
                C('/') => {
                    self.temp.clear();
                    self.state = State::RcdataEndTagOpen;
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::Rcdata);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rcdata-end-tag-open-state
            State::RcdataEndTagOpen => match consume!(self) {
                c if c.is_ascii_alpha() => {
                    self.new_tag(true);
                    self.reconsume_in(State::RcdataEndTagName);
                }
                _ => {
                    self.chars.push_str("</");
                    self.reconsume_in(State::Rcdata);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rcdata-end-tag-name-state
            State::RcdataEndTagName => {
                let c = consume!(self);
                self.end_tag_name_state(c, State::Rcdata);
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#rawtext-less-than-sign-state
            State::RawtextLessThanSign => match consume!(self) {
                C('/') => {
                    self.temp.clear();
                    self.state = State::RawtextEndTagOpen;
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::Rawtext);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rawtext-end-tag-open-state
            State::RawtextEndTagOpen => match consume!(self) {
                c if c.is_ascii_alpha() => {
                    self.new_tag(true);
                    self.reconsume_in(State::RawtextEndTagName);
                }
                _ => {
                    self.chars.push_str("</");
                    self.reconsume_in(State::Rawtext);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#rawtext-end-tag-name-state
            State::RawtextEndTagName => {
                let c = consume!(self);
                self.end_tag_name_state(c, State::Rawtext);
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-less-than-sign-state
            State::ScriptDataLessThanSign => match consume!(self) {
                C('/') => {
                    self.temp.clear();
                    self.state = State::ScriptDataEndTagOpen;
                }
                C('!') => {
                    self.state = State::ScriptDataEscapeStart;
                    self.chars.push_str("<!");
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptData);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-end-tag-open-state
            State::ScriptDataEndTagOpen => match consume!(self) {
                c if c.is_ascii_alpha() => {
                    self.new_tag(true);
                    self.reconsume_in(State::ScriptDataEndTagName);
                }
                _ => {
                    self.chars.push_str("</");
                    self.reconsume_in(State::ScriptData);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-end-tag-name-state
            State::ScriptDataEndTagName => {
                let c = consume!(self);
                self.end_tag_name_state(c, State::ScriptData);
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escape-start-state
            State::ScriptDataEscapeStart => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataEscapeStartDash;
                    self.emit_char('-');
                }
                _ => self.reconsume_in(State::ScriptData),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escape-start-dash-state
            State::ScriptDataEscapeStartDash => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emit_char('-');
                }
                _ => self.reconsume_in(State::ScriptData),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-state
            State::ScriptDataEscaped => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataEscapedDash;
                    self.emit_char('-');
                }
                C('<') => self.state = State::ScriptDataEscapedLessThanSign,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-dash-state
            State::ScriptDataEscapedDash => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataEscapedDashDash;
                    self.emit_char('-');
                }
                C('<') => self.state = State::ScriptDataEscapedLessThanSign,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.state = State::ScriptDataEscaped;
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => {
                    self.state = State::ScriptDataEscaped;
                    self.emit_ch(c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-dash-dash-state
            State::ScriptDataEscapedDashDash => match consume!(self) {
                C('-') => self.emit_char('-'),
                C('<') => self.state = State::ScriptDataEscapedLessThanSign,
                C('>') => {
                    self.state = State::ScriptData;
                    self.emit_char('>');
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.state = State::ScriptDataEscaped;
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => {
                    self.state = State::ScriptDataEscaped;
                    self.emit_ch(c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-less-than-sign-state
            State::ScriptDataEscapedLessThanSign => match consume!(self) {
                C('/') => {
                    self.temp.clear();
                    self.state = State::ScriptDataEscapedEndTagOpen;
                }
                c if c.is_ascii_alpha() => {
                    self.temp.clear();
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptDataDoubleEscapeStart);
                }
                _ => {
                    self.emit_char('<');
                    self.reconsume_in(State::ScriptDataEscaped);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-end-tag-open-state
            State::ScriptDataEscapedEndTagOpen => match consume!(self) {
                c if c.is_ascii_alpha() => {
                    self.new_tag(true);
                    self.reconsume_in(State::ScriptDataEscapedEndTagName);
                }
                _ => {
                    self.chars.push_str("</");
                    self.reconsume_in(State::ScriptDataEscaped);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-escaped-end-tag-name-state
            State::ScriptDataEscapedEndTagName => {
                let c = consume!(self);
                self.end_tag_name_state(c, State::ScriptDataEscaped);
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escape-start-state
            State::ScriptDataDoubleEscapeStart => match consume!(self) {
                c @ C('\t' | '\n' | '\x0C' | ' ' | '/' | '>') => {
                    self.state = if self.temp == "script" {
                        State::ScriptDataDoubleEscaped
                    } else {
                        State::ScriptDataEscaped
                    };
                    self.emit_ch(c);
                }
                C(c) if c.is_ascii_alphabetic() => {
                    self.temp.push(c.to_ascii_lowercase());
                    self.emit_char(c);
                }
                _ => self.reconsume_in(State::ScriptDataEscaped),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escaped-state
            State::ScriptDataDoubleEscaped => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataDoubleEscapedDash;
                    self.emit_char('-');
                }
                C('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    self.emit_char('<');
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escaped-dash-state
            State::ScriptDataDoubleEscapedDash => match consume!(self) {
                C('-') => {
                    self.state = State::ScriptDataDoubleEscapedDashDash;
                    self.emit_char('-');
                }
                C('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    self.emit_char('<');
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_ch(c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escaped-dash-dash-state
            State::ScriptDataDoubleEscapedDashDash => match consume!(self) {
                C('-') => self.emit_char('-'),
                C('<') => {
                    self.state = State::ScriptDataDoubleEscapedLessThanSign;
                    self.emit_char('<');
                }
                C('>') => {
                    self.state = State::ScriptData;
                    self.emit_char('>');
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_char('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInScriptHtmlCommentLikeText);
                    self.emit_eof();
                }
                c => {
                    self.state = State::ScriptDataDoubleEscaped;
                    self.emit_ch(c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escaped-less-than-sign-state
            State::ScriptDataDoubleEscapedLessThanSign => match consume!(self) {
                C('/') => {
                    self.temp.clear();
                    self.state = State::ScriptDataDoubleEscapeEnd;
                    self.emit_char('/');
                }
                _ => self.reconsume_in(State::ScriptDataDoubleEscaped),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#script-data-double-escape-end-state
            State::ScriptDataDoubleEscapeEnd => match consume!(self) {
                c @ C('\t' | '\n' | '\x0C' | ' ' | '/' | '>') => {
                    self.state = if self.temp == "script" {
                        State::ScriptDataEscaped
                    } else {
                        State::ScriptDataDoubleEscaped
                    };
                    self.emit_ch(c);
                }
                C(c) if c.is_ascii_alphabetic() => {
                    self.temp.push(c.to_ascii_lowercase());
                    self.emit_char(c);
                }
                _ => self.reconsume_in(State::ScriptDataDoubleEscaped),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#before-attribute-name-state
            State::BeforeAttributeName => match consume!(self) {
                c if c.is_whitespace() => {}
                C('/' | '>') | Eof => self.reconsume_in(State::AfterAttributeName),
                C('=') => {
                    self.error(ErrorCode::UnexpectedEqualsSignBeforeAttributeName);
                    self.start_attribute();
                    self.attr_name.push('=');
                    self.state = State::AttributeName;
                }
                _ => {
                    self.start_attribute();
                    self.reconsume_in(State::AttributeName);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#attribute-name-state
            State::AttributeName => match consume!(self) {
                c if c.is_whitespace() => {
                    self.leave_attribute_name();
                    self.reconsume_in(State::AfterAttributeName);
                }
                C('/' | '>') | Eof => {
                    self.leave_attribute_name();
                    self.reconsume_in(State::AfterAttributeName);
                }
                C('=') => {
                    self.leave_attribute_name();
                    self.state = State::BeforeAttributeValue;
                }
                C(c) if c.is_ascii_uppercase() => self.attr_name.push(c.to_ascii_lowercase()),
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.attr_name.push('\u{FFFD}');
                }
                c => {
                    if matches!(c, C('"' | '\'' | '<')) {
                        self.error(ErrorCode::UnexpectedCharacterInAttributeName);
                    }
                    push_ch(&mut self.attr_name, c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#after-attribute-name-state
            State::AfterAttributeName => match consume!(self) {
                c if c.is_whitespace() => {}
                C('/') => self.state = State::SelfClosingStartTag,
                C('=') => self.state = State::BeforeAttributeValue,
                C('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                _ => {
                    self.start_attribute();
                    self.reconsume_in(State::AttributeName);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#before-attribute-value-state
            State::BeforeAttributeValue => match consume!(self) {
                c if c.is_whitespace() => {}
                C('"') => self.state = State::AttributeValueDoubleQuoted,
                C('\'') => self.state = State::AttributeValueSingleQuoted,
                C('>') => {
                    self.error(ErrorCode::MissingAttributeValue);
                    self.state = State::Data;
                    self.emit_tag();
                }
                _ => self.reconsume_in(State::AttributeValueUnquoted),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(double-quoted)-state
            State::AttributeValueDoubleQuoted => match consume!(self) {
                C('"') => self.state = State::AfterAttributeValueQuoted,
                C('&') => {
                    self.return_state = State::AttributeValueDoubleQuoted;
                    self.state = State::CharacterReference;
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.attr_value.push('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                c => push_ch(&mut self.attr_value, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(single-quoted)-state
            State::AttributeValueSingleQuoted => match consume!(self) {
                C('\'') => self.state = State::AfterAttributeValueQuoted,
                C('&') => {
                    self.return_state = State::AttributeValueSingleQuoted;
                    self.state = State::CharacterReference;
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.attr_value.push('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                c => push_ch(&mut self.attr_value, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#attribute-value-(unquoted)-state
            State::AttributeValueUnquoted => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeAttributeName,
                C('&') => {
                    self.return_state = State::AttributeValueUnquoted;
                    self.state = State::CharacterReference;
                }
                C('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.attr_value.push('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                c => {
                    if matches!(c, C('"' | '\'' | '<' | '=' | '`')) {
                        self.error(ErrorCode::UnexpectedCharacterInUnquotedAttributeValue);
                    }
                    push_ch(&mut self.attr_value, c);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#after-attribute-value-(quoted)-state
            State::AfterAttributeValueQuoted => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeAttributeName,
                C('/') => self.state = State::SelfClosingStartTag,
                C('>') => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::MissingWhitespaceBetweenAttributes);
                    self.reconsume_in(State::BeforeAttributeName);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#self-closing-start-tag-state
            State::SelfClosingStartTag => match consume!(self) {
                C('>') => {
                    self.tag.self_closing = true;
                    self.state = State::Data;
                    self.emit_tag();
                }
                Eof => {
                    self.error(ErrorCode::EofInTag);
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::UnexpectedSolidusInTag);
                    self.reconsume_in(State::BeforeAttributeName);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#bogus-comment-state
            State::BogusComment => match consume!(self) {
                C('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                Eof => {
                    self.emit_comment();
                    self.emit_eof();
                }
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.comment.push('\u{FFFD}');
                }
                c => push_ch(&mut self.comment, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#markup-declaration-open-state
            State::MarkupDeclarationOpen => {
                for (s, case_insensitive) in [("--", false), ("DOCTYPE", true), ("[CDATA[", false)]
                {
                    match self.lookahead(s, case_insensitive) {
                        Lookahead::NoMatch => continue,
                        Lookahead::NeedMoreInput => return Step::Suspend,
                        Lookahead::Match => {}
                    }
                    self.consume_matched(s.len());
                    match s {
                        "--" => {
                            self.comment.clear();
                            self.state = State::CommentStart;
                        }
                        "DOCTYPE" => self.state = State::Doctype,
                        _ if self.cdata_allowed => self.state = State::CdataSection,
                        _ => {
                            self.error(ErrorCode::CdataInHtmlContent);
                            self.comment = Wtf8Buf::from("[CDATA[");
                            self.state = State::BogusComment;
                        }
                    }
                    return Step::Continue;
                }
                // "don't consume anything in the current state": consuming and then
                // reconsuming is the same, and reports any preprocessing error for that
                // character before this one.
                consume!(self);
                self.error(ErrorCode::IncorrectlyOpenedComment);
                self.comment.clear();
                self.reconsume_in(State::BogusComment);
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-start-state
            State::CommentStart => match consume!(self) {
                C('-') => self.state = State::CommentStartDash,
                C('>') => {
                    self.error(ErrorCode::AbruptClosingOfEmptyComment);
                    self.state = State::Data;
                    self.emit_comment();
                }
                _ => self.reconsume_in(State::Comment),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-start-dash-state
            State::CommentStartDash => match consume!(self) {
                C('-') => self.state = State::CommentEnd,
                C('>') => {
                    self.error(ErrorCode::AbruptClosingOfEmptyComment);
                    self.state = State::Data;
                    self.emit_comment();
                }
                Eof => {
                    self.error(ErrorCode::EofInComment);
                    self.emit_comment();
                    self.emit_eof();
                }
                _ => {
                    self.comment.push('-');
                    self.reconsume_in(State::Comment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-state
            State::Comment => match consume!(self) {
                C('<') => {
                    self.comment.push('<');
                    self.state = State::CommentLessThanSign;
                }
                C('-') => self.state = State::CommentEndDash,
                C('\0') => {
                    self.error(ErrorCode::UnexpectedNullCharacter);
                    self.comment.push('\u{FFFD}');
                }
                Eof => {
                    self.error(ErrorCode::EofInComment);
                    self.emit_comment();
                    self.emit_eof();
                }
                c => push_ch(&mut self.comment, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-less-than-sign-state
            State::CommentLessThanSign => match consume!(self) {
                C('!') => {
                    self.comment.push('!');
                    self.state = State::CommentLessThanSignBang;
                }
                C('<') => self.comment.push('<'),
                _ => self.reconsume_in(State::Comment),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-less-than-sign-bang-state
            State::CommentLessThanSignBang => match consume!(self) {
                C('-') => self.state = State::CommentLessThanSignBangDash,
                _ => self.reconsume_in(State::Comment),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-less-than-sign-bang-dash-state
            State::CommentLessThanSignBangDash => match consume!(self) {
                C('-') => self.state = State::CommentLessThanSignBangDashDash,
                _ => self.reconsume_in(State::CommentEndDash),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-less-than-sign-bang-dash-dash-state
            State::CommentLessThanSignBangDashDash => match consume!(self) {
                C('>') | Eof => self.reconsume_in(State::CommentEnd),
                _ => {
                    self.error(ErrorCode::NestedComment);
                    self.reconsume_in(State::CommentEnd);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-end-dash-state
            State::CommentEndDash => match consume!(self) {
                C('-') => self.state = State::CommentEnd,
                Eof => {
                    self.error(ErrorCode::EofInComment);
                    self.emit_comment();
                    self.emit_eof();
                }
                _ => {
                    self.comment.push('-');
                    self.reconsume_in(State::Comment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-end-state
            State::CommentEnd => match consume!(self) {
                C('>') => {
                    self.state = State::Data;
                    self.emit_comment();
                }
                C('!') => self.state = State::CommentEndBang,
                C('-') => self.comment.push('-'),
                Eof => {
                    self.error(ErrorCode::EofInComment);
                    self.emit_comment();
                    self.emit_eof();
                }
                _ => {
                    self.comment.push_str("--");
                    self.reconsume_in(State::Comment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#comment-end-bang-state
            State::CommentEndBang => match consume!(self) {
                C('-') => {
                    self.comment.push_str("--!");
                    self.state = State::CommentEndDash;
                }
                C('>') => {
                    self.error(ErrorCode::IncorrectlyClosedComment);
                    self.state = State::Data;
                    self.emit_comment();
                }
                Eof => {
                    self.error(ErrorCode::EofInComment);
                    self.emit_comment();
                    self.emit_eof();
                }
                _ => {
                    self.comment.push_str("--!");
                    self.reconsume_in(State::Comment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-state
            State::Doctype => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeDoctypeName,
                C('>') => self.reconsume_in(State::BeforeDoctypeName),
                Eof => {
                    self.error(ErrorCode::EofInDoctype);
                    self.doctype = Doctype {
                        force_quirks: true,
                        ..Doctype::default()
                    };
                    self.emit_doctype();
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::MissingWhitespaceBeforeDoctypeName);
                    self.reconsume_in(State::BeforeDoctypeName);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#before-doctype-name-state
            State::BeforeDoctypeName => match consume!(self) {
                c if c.is_whitespace() => {}
                C('>') => {
                    self.error(ErrorCode::MissingDoctypeName);
                    self.doctype = Doctype {
                        force_quirks: true,
                        ..Doctype::default()
                    };
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Eof => {
                    self.error(ErrorCode::EofInDoctype);
                    self.doctype = Doctype {
                        force_quirks: true,
                        ..Doctype::default()
                    };
                    self.emit_doctype();
                    self.emit_eof();
                }
                c => {
                    let mut name = Wtf8Buf::new();
                    match c {
                        C(c) if c.is_ascii_uppercase() => name.push(c.to_ascii_lowercase()),
                        C('\0') => {
                            self.error(ErrorCode::UnexpectedNullCharacter);
                            name.push('\u{FFFD}');
                        }
                        c => push_ch(&mut name, c),
                    }
                    self.doctype = Doctype {
                        name: Some(name),
                        ..Doctype::default()
                    };
                    self.state = State::DoctypeName;
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-name-state
            State::DoctypeName => {
                let c = consume!(self);
                let name = self.doctype.name.get_or_insert_default();
                match c {
                    c if c.is_whitespace() => self.state = State::AfterDoctypeName,
                    C('>') => {
                        self.state = State::Data;
                        self.emit_doctype();
                    }
                    C(c) if c.is_ascii_uppercase() => name.push(c.to_ascii_lowercase()),
                    C('\0') => {
                        name.push('\u{FFFD}');
                        self.error(ErrorCode::UnexpectedNullCharacter);
                    }
                    Eof => self.eof_in_doctype(),
                    c => push_ch(name, c),
                }
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#after-doctype-name-state
            State::AfterDoctypeName => match consume!(self) {
                c if c.is_whitespace() => {}
                C('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    // the six characters starting from the current input character.
                    self.reconsume = true;
                    let mut keyword = None;
                    for (s, next) in [
                        ("PUBLIC", State::AfterDoctypePublicKeyword),
                        ("SYSTEM", State::AfterDoctypeSystemKeyword),
                    ] {
                        match self.lookahead(s, true) {
                            Lookahead::NoMatch => {}
                            Lookahead::NeedMoreInput => return Step::Suspend,
                            Lookahead::Match => {
                                keyword = Some(next);
                                break;
                            }
                        }
                    }
                    match keyword {
                        Some(next) => {
                            self.consume_matched(6);
                            self.state = next;
                        }
                        None => {
                            self.error(ErrorCode::InvalidCharacterSequenceAfterDoctypeName);
                            self.doctype.force_quirks = true;
                            self.reconsume_in(State::BogusDoctype);
                        }
                    }
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#after-doctype-public-keyword-state
            State::AfterDoctypePublicKeyword => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeDoctypePublicIdentifier,
                C(q @ ('"' | '\'')) => {
                    self.error(ErrorCode::MissingWhitespaceAfterDoctypePublicKeyword);
                    self.start_public_id(q);
                }
                C('>') => {
                    self.error(ErrorCode::MissingDoctypePublicIdentifier);
                    self.doctype_closed_early();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypePublicIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#before-doctype-public-identifier-state
            State::BeforeDoctypePublicIdentifier => match consume!(self) {
                c if c.is_whitespace() => {}
                C(q @ ('"' | '\'')) => self.start_public_id(q),
                C('>') => {
                    self.error(ErrorCode::MissingDoctypePublicIdentifier);
                    self.doctype_closed_early();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypePublicIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-public-identifier-(double-quoted)-state
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-public-identifier-(single-quoted)-state
            State::DoctypePublicIdentifierDoubleQuoted
            | State::DoctypePublicIdentifierSingleQuoted => {
                let quote = if self.state == State::DoctypePublicIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                let c = consume!(self);
                let id = self.doctype.public_id.get_or_insert_default();
                match c {
                    C(q) if q == quote => self.state = State::AfterDoctypePublicIdentifier,
                    C('\0') => {
                        id.push('\u{FFFD}');
                        self.error(ErrorCode::UnexpectedNullCharacter);
                    }
                    C('>') => {
                        self.error(ErrorCode::AbruptDoctypePublicIdentifier);
                        self.doctype_closed_early();
                    }
                    Eof => self.eof_in_doctype(),
                    c => push_ch(id, c),
                }
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#after-doctype-public-identifier-state
            State::AfterDoctypePublicIdentifier => match consume!(self) {
                c if c.is_whitespace() => {
                    self.state = State::BetweenDoctypePublicAndSystemIdentifiers;
                }
                C('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                C(q @ ('"' | '\'')) => {
                    self.error(
                        ErrorCode::MissingWhitespaceBetweenDoctypePublicAndSystemIdentifiers,
                    );
                    self.start_system_id(q);
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#between-doctype-public-and-system-identifiers-state
            State::BetweenDoctypePublicAndSystemIdentifiers => match consume!(self) {
                c if c.is_whitespace() => {}
                C('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                C(q @ ('"' | '\'')) => self.start_system_id(q),
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#after-doctype-system-keyword-state
            State::AfterDoctypeSystemKeyword => match consume!(self) {
                c if c.is_whitespace() => self.state = State::BeforeDoctypeSystemIdentifier,
                C(q @ ('"' | '\'')) => {
                    self.error(ErrorCode::MissingWhitespaceAfterDoctypeSystemKeyword);
                    self.start_system_id(q);
                }
                C('>') => {
                    self.error(ErrorCode::MissingDoctypeSystemIdentifier);
                    self.doctype_closed_early();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#before-doctype-system-identifier-state
            State::BeforeDoctypeSystemIdentifier => match consume!(self) {
                c if c.is_whitespace() => {}
                C(q @ ('"' | '\'')) => self.start_system_id(q),
                C('>') => {
                    self.error(ErrorCode::MissingDoctypeSystemIdentifier);
                    self.doctype_closed_early();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    self.error(ErrorCode::MissingQuoteBeforeDoctypeSystemIdentifier);
                    self.doctype.force_quirks = true;
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-system-identifier-(double-quoted)-state
            // https://html.spec.whatwg.org/multipage/parsing.html#doctype-system-identifier-(single-quoted)-state
            State::DoctypeSystemIdentifierDoubleQuoted
            | State::DoctypeSystemIdentifierSingleQuoted => {
                let quote = if self.state == State::DoctypeSystemIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                let c = consume!(self);
                let id = self.doctype.system_id.get_or_insert_default();
                match c {
                    C(q) if q == quote => self.state = State::AfterDoctypeSystemIdentifier,
                    C('\0') => {
                        id.push('\u{FFFD}');
                        self.error(ErrorCode::UnexpectedNullCharacter);
                    }
                    C('>') => {
                        self.error(ErrorCode::AbruptDoctypeSystemIdentifier);
                        self.doctype_closed_early();
                    }
                    Eof => self.eof_in_doctype(),
                    c => push_ch(id, c),
                }
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#after-doctype-system-identifier-state
            State::AfterDoctypeSystemIdentifier => match consume!(self) {
                c if c.is_whitespace() => {}
                C('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                Eof => self.eof_in_doctype(),
                _ => {
                    // this doesn't set the force-quirks flag.
                    self.error(ErrorCode::UnexpectedCharacterAfterDoctypeSystemIdentifier);
                    self.reconsume_in(State::BogusDoctype);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#bogus-doctype-state
            State::BogusDoctype => match consume!(self) {
                C('>') => {
                    self.state = State::Data;
                    self.emit_doctype();
                }
                C('\0') => self.error(ErrorCode::UnexpectedNullCharacter),
                Eof => {
                    self.emit_doctype();
                    self.emit_eof();
                }
                _ => {}
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#cdata-section-state
            State::CdataSection => match consume!(self) {
                C(']') => self.state = State::CdataSectionBracket,
                Eof => {
                    self.error(ErrorCode::EofInCdata);
                    self.emit_eof();
                }
                c => self.emit_ch(c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#cdata-section-bracket-state
            State::CdataSectionBracket => match consume!(self) {
                C(']') => self.state = State::CdataSectionEnd,
                _ => {
                    self.emit_char(']');
                    self.reconsume_in(State::CdataSection);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#cdata-section-end-state
            State::CdataSectionEnd => match consume!(self) {
                C(']') => self.emit_char(']'),
                C('>') => self.state = State::Data,
                _ => {
                    self.chars.push_str("]]");
                    self.reconsume_in(State::CdataSection);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#processing-instruction-open-state
            State::ProcessingInstructionOpen => match consume!(self) {
                C(c) if c.is_ascii_alphabetic() || c == '_' => {
                    self.reconsume_in(State::ProcessingInstructionTarget);
                }
                Eof => {
                    self.error(ErrorCode::EofInProcessingInstruction);
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::InvalidFirstCharacterOfProcessingInstructionTarget);
                    self.temp_to_comment();
                    self.reconsume_in(State::BogusComment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#processing-instruction-target-state
            State::ProcessingInstructionTarget => match consume!(self) {
                C('\t' | '\n' | '\x0C' | ' ' | '?' | '>') => {
                    if self.temp.eq_ignore_ascii_case("xml")
                        || self.temp.eq_ignore_ascii_case("xml-stylesheet")
                    {
                        self.error(ErrorCode::DisallowedProcessingInstructionTarget);
                        self.temp_to_comment();
                        self.reconsume_in(State::BogusComment);
                    } else {
                        self.pi = ProcessingInstruction {
                            target: self.temp.clone(),
                            data: Wtf8Buf::new(),
                        };
                        self.reconsume_in(State::AfterProcessingInstructionTarget);
                    }
                }
                C(c) if c.is_ascii_alphanumeric() || c == '-' || c == '_' => self.temp.push(c),
                Eof => {
                    self.error(ErrorCode::EofInProcessingInstruction);
                    self.emit_eof();
                }
                _ => {
                    self.error(ErrorCode::InvalidProcessingInstructionTarget);
                    self.temp_to_comment();
                    self.reconsume_in(State::BogusComment);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#after-processing-instruction-target-state
            State::AfterProcessingInstructionTarget => match consume!(self) {
                c if c.is_whitespace() => {}
                _ => self.reconsume_in(State::ProcessingInstructionData),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#processing-instruction-data-state
            State::ProcessingInstructionData => match consume!(self) {
                C('?') => self.state = State::ProcessingInstructionQuestionable,
                C('>') => {
                    self.state = State::Data;
                    self.emit_processing_instruction();
                }
                Eof => {
                    self.error(ErrorCode::EofInProcessingInstruction);
                    self.emit_eof();
                }
                c => push_ch(&mut self.pi.data, c),
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#processing-instruction-questionable-state
            State::ProcessingInstructionQuestionable => match consume!(self) {
                C('>') => {
                    self.state = State::Data;
                    self.emit_processing_instruction();
                }
                Eof => {
                    self.error(ErrorCode::EofInProcessingInstruction);
                    self.emit_eof();
                }
                _ => {
                    self.pi.data.push('?');
                    self.reconsume_in(State::ProcessingInstructionData);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#character-reference-state
            State::CharacterReference => {
                self.temp.clear();
                self.temp.push('&');
                match consume!(self) {
                    c if c.is_ascii_alphanumeric() => {
                        self.reconsume_in(State::NamedCharacterReference);
                    }
                    C('#') => {
                        self.temp.push('#');
                        self.state = State::NumericCharacterReference;
                    }
                    _ => {
                        self.flush_char_ref();
                        let state = self.return_state;
                        self.reconsume_in(state);
                    }
                }
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#named-character-reference-state
            State::NamedCharacterReference => return self.named_character_reference(),
            // https://html.spec.whatwg.org/multipage/parsing.html#ambiguous-ampersand-state
            State::AmbiguousAmpersand => match consume!(self) {
                c if c.is_ascii_alphanumeric() => {
                    if self.char_ref_in_attribute() {
                        push_ch(&mut self.attr_value, c);
                    } else {
                        self.emit_ch(c);
                    }
                }
                C(';') => {
                    self.error(ErrorCode::UnknownNamedCharacterReference);
                    let state = self.return_state;
                    self.reconsume_in(state);
                }
                _ => {
                    let state = self.return_state;
                    self.reconsume_in(state);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-state
            State::NumericCharacterReference => {
                self.char_ref_code = 0;
                match consume!(self) {
                    C(c @ ('x' | 'X')) => {
                        self.temp.push(c);
                        self.state = State::HexadecimalCharacterReferenceStart;
                    }
                    C(c) if c.is_ascii_digit() => {
                        self.reconsume_in(State::DecimalCharacterReference);
                    }
                    _ => {
                        self.error(ErrorCode::AbsenceOfDigitsInNumericCharacterReference);
                        self.flush_char_ref();
                        let state = self.return_state;
                        self.reconsume_in(state);
                    }
                }
            }
            // https://html.spec.whatwg.org/multipage/parsing.html#hexadecimal-character-reference-start-state
            State::HexadecimalCharacterReferenceStart => match consume!(self) {
                C(c) if c.is_ascii_hexdigit() => {
                    self.reconsume_in(State::HexadecimalCharacterReference);
                }
                _ => {
                    self.error(ErrorCode::AbsenceOfDigitsInNumericCharacterReference);
                    self.flush_char_ref();
                    let state = self.return_state;
                    self.reconsume_in(state);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#hexadecimal-character-reference-state
            State::HexadecimalCharacterReference => match consume!(self) {
                C(c) if c.is_ascii_hexdigit() => {
                    let digit = c.to_digit(16).unwrap_or(0);
                    self.add_char_ref_digit(16, digit);
                }
                C(';') => self.state = State::NumericCharacterReferenceEnd,
                _ => {
                    self.error(ErrorCode::MissingSemicolonAfterCharacterReference);
                    self.reconsume_in(State::NumericCharacterReferenceEnd);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#decimal-character-reference-state
            State::DecimalCharacterReference => match consume!(self) {
                C(c) if c.is_ascii_digit() => {
                    let digit = c.to_digit(10).unwrap_or(0);
                    self.add_char_ref_digit(10, digit);
                }
                C(';') => self.state = State::NumericCharacterReferenceEnd,
                _ => {
                    self.error(ErrorCode::MissingSemicolonAfterCharacterReference);
                    self.reconsume_in(State::NumericCharacterReferenceEnd);
                }
            },
            // https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state
            State::NumericCharacterReferenceEnd => self.numeric_character_reference_end(),
        }
        Step::Continue
    }

    fn eof_in_doctype(&mut self) {
        self.error(ErrorCode::EofInDoctype);
        self.doctype.force_quirks = true;
        self.emit_doctype();
        self.emit_eof();
    }

    /// The shared "set force-quirks, switch to the data state, emit the DOCTYPE" for a
    /// DOCTYPE that ends before its identifiers do.
    fn doctype_closed_early(&mut self) {
        self.doctype.force_quirks = true;
        self.state = State::Data;
        self.emit_doctype();
    }

    fn start_public_id(&mut self, quote: char) {
        self.doctype.public_id = Some(Wtf8Buf::new());
        self.state = if quote == '"' {
            State::DoctypePublicIdentifierDoubleQuoted
        } else {
            State::DoctypePublicIdentifierSingleQuoted
        };
    }

    fn start_system_id(&mut self, quote: char) {
        self.doctype.system_id = Some(Wtf8Buf::new());
        self.state = if quote == '"' {
            State::DoctypeSystemIdentifierDoubleQuoted
        } else {
            State::DoctypeSystemIdentifierSingleQuoted
        };
    }

    fn emit_processing_instruction(&mut self) {
        let pi = mem::take(&mut self.pi);
        self.emit(Token::ProcessingInstruction(pi));
    }

    fn add_char_ref_digit(&mut self, base: u32, digit: u32) {
        // anything above 0x10FFFF ends up as U+FFFD, so stop growing there rather than
        // overflow.
        self.char_ref_code = (self.char_ref_code * base + digit).min(0x11_0000);
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#named-character-reference-state
    fn named_character_reference(&mut self) -> Step {
        // "consume the maximum number of characters possible, where the consumed
        // characters are one of the identifiers in the first column of the named
        // character references table". Look ahead first, so that running out of input
        // mid-name suspends without consuming anything.
        let mut matcher = NameMatcher::new();
        let mut seen = Vec::new();
        for peeked in self.peek() {
            seen.push(peeked);
            match peeked {
                Peeked::Char(cp) => {
                    let c = char::from_u32(cp).unwrap_or('\u{FFFD}');
                    if !matcher.push(c) {
                        break;
                    }
                }
                Peeked::Eof => break,
                Peeked::End => return Step::Suspend,
            }
        }
        let Some((len, value)) = matcher.longest() else {
            // no match: flush the "&" and let the ambiguous ampersand state consume the
            // characters.
            self.flush_char_ref();
            self.state = State::AmbiguousAmpersand;
            return Step::Continue;
        };
        let ends_with_semicolon = value_name_ends_with_semicolon(&seen[..len]);
        let next = seen[len];
        for &peeked in &seen[..len] {
            if let Peeked::Char(cp) = peeked
                && let Some(c) = char::from_u32(cp)
            {
                self.temp.push(c);
            }
        }
        self.consume_matched(len);
        let next_is_equals_or_alnum = matches!(
            next,
            Peeked::Char(cp) if char::from_u32(cp).is_some_and(|c| c == '=' || c.is_ascii_alphanumeric())
        );
        if self.char_ref_in_attribute() && !ends_with_semicolon && next_is_equals_or_alnum {
            // for historical reasons, the reference stays as written.
            self.flush_char_ref();
            self.state = self.return_state;
            return Step::Continue;
        }
        if !ends_with_semicolon {
            self.error_at_next_char(ErrorCode::MissingSemicolonAfterCharacterReference);
        }
        self.temp.clear();
        self.temp.push_str(value);
        self.flush_char_ref();
        self.state = self.return_state;
        Step::Continue
    }

    // https://html.spec.whatwg.org/multipage/parsing.html#numeric-character-reference-end-state
    fn numeric_character_reference_end(&mut self) {
        let mut code = self.char_ref_code;
        if code == 0 {
            self.error_at_next_char(ErrorCode::NullCharacterReference);
            code = 0xFFFD;
        }
        if code > 0x10FFFF {
            self.error_at_next_char(ErrorCode::CharacterReferenceOutsideUnicodeRange);
            code = 0xFFFD;
        }
        if (0xD800..=0xDFFF).contains(&code) {
            self.error_at_next_char(ErrorCode::SurrogateCharacterReference);
            code = 0xFFFD;
        }
        if is_noncharacter(code) {
            self.error_at_next_char(ErrorCode::NoncharacterCharacterReference);
        }
        if code == 0x0D || (is_control(code) && !matches!(code, 0x09 | 0x0A | 0x0C | 0x20)) {
            self.error_at_next_char(ErrorCode::ControlCharacterReference);
            if let Some(mapped) = c1_replacement(code) {
                code = mapped;
            }
        }
        self.temp.clear();
        self.temp
            .push(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
        self.flush_char_ref();
        self.state = self.return_state;
    }
}

fn value_name_ends_with_semicolon(matched: &[Peeked]) -> bool {
    matched.last() == Some(&Peeked::Char(u32::from(';')))
}

// https://infra.spec.whatwg.org/#noncharacter
fn is_noncharacter(cp: u32) -> bool {
    (0xFDD0..=0xFDEF).contains(&cp) || (cp <= 0x10FFFF && (cp & 0xFFFE) == 0xFFFE)
}

// https://infra.spec.whatwg.org/#control
fn is_control(cp: u32) -> bool {
    cp <= 0x1F || (0x7F..=0x9F).contains(&cp)
}

// the table in the numeric character reference end state.
fn c1_replacement(code: u32) -> Option<u32> {
    Some(match code {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
