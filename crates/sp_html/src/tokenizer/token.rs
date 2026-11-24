use crate::Wtf8Buf;

// https://html.spec.whatwg.org/multipage/parsing.html#tokenization
// adjacent character tokens are coalesced into one `Characters` token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Doctype(Doctype),
    StartTag(Tag),
    EndTag(Tag),
    Comment(Wtf8Buf),
    ProcessingInstruction(ProcessingInstruction),
    Characters(Wtf8Buf),
    Eof,
}

/// A DOCTYPE token. `None` is the spec's "missing", distinct from the empty string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doctype {
    pub name: Option<Wtf8Buf>,
    pub public_id: Option<Wtf8Buf>,
    pub system_id: Option<Wtf8Buf>,
    pub force_quirks: bool,
}

/// A start or end tag token. Duplicate attributes have already been removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag {
    pub name: Wtf8Buf,
    pub self_closing: bool,
    pub attributes: Vec<Attribute>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub name: Wtf8Buf,
    pub value: Wtf8Buf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessingInstruction {
    pub target: String,
    pub data: Wtf8Buf,
}
