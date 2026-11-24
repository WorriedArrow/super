//! strings of code points stored as wtf-8 (https://simonsapin.github.io/wtf-8/).
//!
//! dom strings are sequences of utf-16 code units, so text, comments and
//! attribute values can hold lone surrogates: the html input stream keeps
//! them (https://html.spec.whatwg.org/multipage/parsing.html#preprocessing-the-input-stream)
//! and javascript strings can contain them. wtf-8 is utf-8 extended to
//! encode lone surrogates, with a surrogate pair always stored as the
//! supplementary code point, so text that is valid unicode is plain utf-8.
//! conversion to utf-16 happens only at the javascript boundary.

use std::borrow::Cow;
use std::fmt;
use std::ops::Range;

/// an owned wtf-8 string.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Wtf8Buf {
    bytes: Vec<u8>,
}

/// a borrowed wtf-8 string.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Wtf8<'a> {
    bytes: &'a [u8],
}

fn is_surrogate(cp: u32) -> bool {
    (0xD800..=0xDFFF).contains(&cp)
}

fn is_low_surrogate(cp: u32) -> bool {
    (0xDC00..=0xDFFF).contains(&cp)
}

impl Wtf8Buf {
    pub const fn new() -> Self {
        Wtf8Buf { bytes: Vec::new() }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Wtf8Buf {
            bytes: Vec::with_capacity(capacity),
        }
    }

    /// decodes utf-16, keeping unpaired surrogates.
    pub fn from_utf16(units: &[u16]) -> Self {
        let mut buf = Wtf8Buf::with_capacity(units.len());
        for unit in char::decode_utf16(units.iter().copied()) {
            match unit {
                Ok(c) => buf.push(c),
                Err(err) => buf.push_code_point(u32::from(err.unpaired_surrogate())),
            }
        }
        buf
    }

    pub fn as_wtf8(&self) -> Wtf8<'_> {
        Wtf8 { bytes: &self.bytes }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }

    pub fn push(&mut self, c: char) {
        let mut tmp = [0; 4];
        self.bytes
            .extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
    }

    /// a `str` never starts with a lone low surrogate, so nothing joins.
    pub fn push_str(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
    }

    /// appends any code point, including a surrogate. a low surrogate that
    /// follows a trailing high surrogate joins it into one supplementary code
    /// point, as wtf-8 requires. values above U+10FFFF append U+FFFD.
    pub fn push_code_point(&mut self, cp: u32) {
        if let Some(c) = char::from_u32(cp) {
            self.push(c);
            return;
        }
        if !is_surrogate(cp) {
            self.push(char::REPLACEMENT_CHARACTER);
            return;
        }
        if is_low_surrogate(cp)
            && let Some(high) = self.trailing_high_surrogate()
        {
            self.bytes.truncate(self.bytes.len() - 3);
            self.push_code_point(0x10000 + ((high - 0xD800) << 10) + (cp - 0xDC00));
            return;
        }
        // surrogates are three-byte sequences starting with 0xED.
        self.bytes.extend_from_slice(&[
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    }

    /// appends another wtf-8 string (or a `&str`, or a `&Wtf8Buf`), joining a
    /// trailing high surrogate here with a leading low surrogate there.
    pub fn push_wtf8<'a>(&mut self, other: impl Into<Wtf8<'a>>) {
        let other = other.into();
        match other.code_points().next() {
            Some((cp, len)) if is_low_surrogate(cp) => {
                self.push_code_point(cp);
                self.bytes.extend_from_slice(&other.bytes[len..]);
            }
            _ => self.bytes.extend_from_slice(other.bytes),
        }
    }

    fn trailing_high_surrogate(&self) -> Option<u32> {
        match self.bytes.as_slice() {
            [.., 0xED, b1 @ 0xA0..=0xAF, b2] => {
                Some(0xD000 | (u32::from(*b1 & 0x3F) << 6) | u32::from(*b2 & 0x3F))
            }
            _ => None,
        }
    }

    /// the contents as a `str`, or none if they contain a lone surrogate.
    pub fn as_str(&self) -> Option<&str> {
        self.as_wtf8().as_str()
    }

    /// the contents with each lone surrogate replaced by U+FFFD.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        self.as_wtf8().to_string_lossy()
    }

    /// the code points, each with its encoded length in bytes.
    pub fn code_points(&self) -> CodePoints<'_> {
        self.as_wtf8().code_points()
    }

    /// the code point starting at byte `index`, with its length.
    pub fn code_point_at(&self, index: usize) -> Option<(u32, usize)> {
        self.as_wtf8().code_point_at(index)
    }
}

impl<'a> Wtf8<'a> {
    pub fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    pub fn len(self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(self) -> bool {
        self.bytes.is_empty()
    }

    /// the contents as a `str`, or none if they contain a lone surrogate.
    pub fn as_str(self) -> Option<&'a str> {
        std::str::from_utf8(self.bytes).ok()
    }

    /// the contents with each lone surrogate replaced by U+FFFD.
    pub fn to_string_lossy(self) -> Cow<'a, str> {
        match self.as_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(
                self.code_points()
                    .map(|(cp, _)| char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect(),
            ),
        }
    }

    pub fn to_wtf8_buf(self) -> Wtf8Buf {
        Wtf8Buf {
            bytes: self.bytes.to_vec(),
        }
    }

    /// the code points, each with its encoded length in bytes.
    pub fn code_points(self) -> CodePoints<'a> {
        CodePoints { bytes: self.bytes }
    }

    /// the code point starting at byte `index`, with its length.
    pub fn code_point_at(self, index: usize) -> Option<(u32, usize)> {
        decode(self.bytes.get(index..)?)
    }

    pub fn is_code_point_boundary(self, index: usize) -> bool {
        match self.bytes.get(index) {
            Some(&b) => !(0x80..=0xBF).contains(&b),
            None => index == self.bytes.len(),
        }
    }

    /// the bytes in `range`. panics unless both ends are code point
    /// boundaries.
    pub fn slice(self, range: Range<usize>) -> Wtf8<'a> {
        assert!(
            self.is_code_point_boundary(range.start) && self.is_code_point_boundary(range.end),
            "wtf-8 slice {range:?} not on code point boundaries"
        );
        Wtf8 {
            bytes: &self.bytes[range],
        }
    }

    /// splits on ascii whitespace bytes, dropping empty pieces. ascii bytes
    /// never occur inside a multi-byte sequence, so each piece is wtf-8.
    pub fn split_ascii_whitespace(self) -> impl Iterator<Item = Wtf8<'a>> {
        self.bytes
            .split(|b| matches!(b, b'\t' | b'\n' | b'\x0C' | b'\r' | b' '))
            .filter(|piece| !piece.is_empty())
            .map(|bytes| Wtf8 { bytes })
    }
}

/// decodes the first code point of well-formed wtf-8 and returns it with its
/// length.
fn decode(bytes: &[u8]) -> Option<(u32, usize)> {
    let first = *bytes.first()?;
    let cont = |i: usize| u32::from(bytes[i] & 0x3F);
    Some(match first {
        0x00..=0x7F => (u32::from(first), 1),
        0xC0..=0xDF => ((u32::from(first & 0x1F) << 6) | cont(1), 2),
        0xE0..=0xEF => (
            (u32::from(first & 0x0F) << 12) | (cont(1) << 6) | cont(2),
            3,
        ),
        _ => (
            (u32::from(first & 0x07) << 18) | (cont(1) << 12) | (cont(2) << 6) | cont(3),
            4,
        ),
    })
}

pub struct CodePoints<'a> {
    bytes: &'a [u8],
}

impl Iterator for CodePoints<'_> {
    type Item = (u32, usize);

    fn next(&mut self) -> Option<(u32, usize)> {
        let (cp, len) = decode(self.bytes)?;
        self.bytes = &self.bytes[len..];
        Some((cp, len))
    }
}

impl From<&str> for Wtf8Buf {
    fn from(s: &str) -> Self {
        Wtf8Buf {
            bytes: s.as_bytes().to_vec(),
        }
    }
}

impl From<String> for Wtf8Buf {
    fn from(s: String) -> Self {
        Wtf8Buf {
            bytes: s.into_bytes(),
        }
    }
}

impl From<Wtf8<'_>> for Wtf8Buf {
    fn from(s: Wtf8<'_>) -> Self {
        s.to_wtf8_buf()
    }
}

impl<'a> From<&'a str> for Wtf8<'a> {
    fn from(s: &'a str) -> Self {
        Wtf8 {
            bytes: s.as_bytes(),
        }
    }
}

impl<'a> From<&'a Wtf8Buf> for Wtf8<'a> {
    fn from(s: &'a Wtf8Buf) -> Self {
        s.as_wtf8()
    }
}

impl PartialEq<str> for Wtf8Buf {
    fn eq(&self, other: &str) -> bool {
        self.bytes == other.as_bytes()
    }
}

impl PartialEq<&str> for Wtf8Buf {
    fn eq(&self, other: &&str) -> bool {
        self.bytes == other.as_bytes()
    }
}

impl PartialEq<str> for Wtf8<'_> {
    fn eq(&self, other: &str) -> bool {
        self.bytes == other.as_bytes()
    }
}

impl PartialEq<&str> for Wtf8<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.bytes == other.as_bytes()
    }
}

impl fmt::Debug for Wtf8<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for (cp, _) in self.code_points() {
            match char::from_u32(cp) {
                Some(c) => write!(f, "{}", c.escape_debug())?,
                None => write!(f, "\\u{{{cp:x}}}")?,
            }
        }
        f.write_str("\"")
    }
}

impl fmt::Debug for Wtf8Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.as_wtf8(), f)
    }
}

impl fmt::Display for Wtf8<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl fmt::Display for Wtf8Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.as_wtf8(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_surrogates_round_trip() {
        let buf = Wtf8Buf::from_utf16(&[0x61, 0xD800, 0x62, 0xDFFF]);
        let cps: Vec<u32> = buf.code_points().map(|(cp, _)| cp).collect();
        assert_eq!(cps, [0x61, 0xD800, 0x62, 0xDFFF]);
        assert_eq!(buf.as_str(), None);
        assert_eq!(buf.to_string_lossy(), "a\u{FFFD}b\u{FFFD}");
        assert_eq!(format!("{buf:?}"), "\"a\\u{d800}b\\u{dfff}\"");
    }

    #[test]
    fn surrogate_pair_joins() {
        let mut buf = Wtf8Buf::new();
        buf.push_code_point(0xD83D);
        buf.push_code_point(0xDE00);
        assert_eq!(buf, "\u{1F600}");

        let mut a = Wtf8Buf::from_utf16(&[0xD83D]);
        a.push_wtf8(&Wtf8Buf::from_utf16(&[0xDE00, 0x61]));
        assert_eq!(a, "\u{1F600}a");
        a.push_wtf8("b");
        assert_eq!(a, "\u{1F600}ab");
        a.push_code_point(0x11_0000);
        assert_eq!(a, "\u{1F600}ab\u{FFFD}");
    }

    #[test]
    fn valid_text_is_utf8() {
        let mut buf = Wtf8Buf::from("x");
        buf.push('\u{E9}');
        buf.push_str("\u{10000}");
        assert_eq!(buf.as_str(), Some("x\u{E9}\u{10000}"));
        assert_eq!(format!("{buf:?}"), "\"x\u{E9}\u{10000}\"");
    }

    #[test]
    fn slicing_and_splitting() {
        let buf = Wtf8Buf::from_utf16(&[0x20, 0xD800, 0x20, 0x20, 0xE9, 0x09]);
        let pieces: Vec<Wtf8Buf> = buf
            .as_wtf8()
            .split_ascii_whitespace()
            .map(Wtf8Buf::from)
            .collect();
        assert_eq!(
            pieces,
            [Wtf8Buf::from_utf16(&[0xD800]), Wtf8Buf::from("\u{E9}")]
        );
        let w = buf.as_wtf8();
        assert!(w.is_code_point_boundary(1));
        assert!(!w.is_code_point_boundary(2));
        assert_eq!(w.slice(1..4).code_points().next(), Some((0xD800, 3)));
        assert_eq!(w.code_point_at(6), Some((0xE9, 2)));
    }

    #[test]
    #[should_panic(expected = "code point boundaries")]
    fn slicing_inside_a_code_point_panics() {
        Wtf8::from("\u{E9}").slice(0..1);
    }
}
