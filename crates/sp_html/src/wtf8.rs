//! A growable string of code points, stored as WTF-8.
//!
//! The html input stream is a sequence of code points, not scalar values:
//! https://html.spec.whatwg.org/multipage/parsing.html#preprocessing-the-input-stream
//! reports surrogates as parse errors but keeps them, and `document.write()` can insert
//! lone surrogates from a javascript string. A rust `String` can't hold those, so token
//! data uses WTF-8 (https://simonsapin.github.io/wtf-8/): UTF-8 extended to encode lone
//! surrogates, with a surrogate pair always stored as the supplementary code point.

use std::borrow::Cow;
use std::fmt;

#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Wtf8Buf {
    bytes: Vec<u8>,
}

impl Wtf8Buf {
    pub const fn new() -> Self {
        Wtf8Buf { bytes: Vec::new() }
    }

    /// Decodes UTF-16, keeping unpaired surrogates.
    pub fn from_utf16(units: &[u16]) -> Self {
        let mut buf = Wtf8Buf::new();
        for unit in char::decode_utf16(units.iter().copied()) {
            match unit {
                Ok(c) => buf.push(c),
                Err(err) => buf.push_code_point(u32::from(err.unpaired_surrogate())),
            }
        }
        buf
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

    pub fn push_str(&mut self, s: &str) {
        self.bytes.extend_from_slice(s.as_bytes());
    }

    /// Appends any code point, including a surrogate. A low surrogate that follows a
    /// trailing high surrogate joins it into one supplementary code point, as WTF-8
    /// requires. Values above U+10FFFF append U+FFFD.
    pub fn push_code_point(&mut self, cp: u32) {
        if let Some(c) = char::from_u32(cp) {
            self.push(c);
            return;
        }
        if !(0xD800..=0xDFFF).contains(&cp) {
            self.push(char::REPLACEMENT_CHARACTER);
            return;
        }
        if (0xDC00..=0xDFFF).contains(&cp)
            && let Some(high) = self.trailing_high_surrogate()
        {
            self.bytes.truncate(self.bytes.len() - 3);
            let joined = 0x10000 + ((high - 0xD800) << 10) + (cp - 0xDC00);
            self.push_code_point(joined);
            return;
        }
        // surrogates are three-byte sequences starting with 0xED.
        self.bytes.extend_from_slice(&[
            0xE0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3F) as u8,
            0x80 | (cp & 0x3F) as u8,
        ]);
    }

    pub fn push_wtf8(&mut self, other: &Wtf8Buf) {
        match other.code_points().next() {
            Some((cp, len)) if (0xDC00..=0xDFFF).contains(&cp) => {
                self.push_code_point(cp);
                self.bytes.extend_from_slice(&other.bytes[len..]);
            }
            _ => self.bytes.extend_from_slice(&other.bytes),
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

    /// The contents as a `str`, or `None` if they contain a lone surrogate.
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }

    /// The contents with each lone surrogate replaced by U+FFFD.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match self.as_str() {
            Some(s) => Cow::Borrowed(s),
            None => Cow::Owned(
                self.code_points()
                    .map(|(cp, _)| char::from_u32(cp).unwrap_or(char::REPLACEMENT_CHARACTER))
                    .collect(),
            ),
        }
    }

    /// Iterates over the code points, each with its encoded length in bytes.
    pub fn code_points(&self) -> CodePoints<'_> {
        CodePoints { bytes: &self.bytes }
    }

    /// `bytes` must be a slice of a `Wtf8Buf` cut at code point boundaries.
    pub(crate) fn from_wtf8_bytes(bytes: &[u8]) -> Self {
        Wtf8Buf {
            bytes: bytes.to_vec(),
        }
    }

    pub(crate) fn code_point_at(&self, index: usize) -> Option<(u32, usize)> {
        decode(&self.bytes[index..])
    }
}

/// Decodes the first code point of well-formed WTF-8 and returns it with its length.
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

impl fmt::Debug for Wtf8Buf {
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

impl fmt::Display for Wtf8Buf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
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
    }

    #[test]
    fn valid_text_is_utf8() {
        let mut buf = Wtf8Buf::from("x");
        buf.push('\u{E9}');
        buf.push_str("\u{10000}");
        assert_eq!(buf.as_str(), Some("x\u{E9}\u{10000}"));
        assert_eq!(format!("{buf:?}"), "\"x\u{E9}\u{10000}\"");
    }
}
