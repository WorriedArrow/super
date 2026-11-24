//! https://drafts.csswg.org/css-syntax-3/#input-byte-stream

use std::borrow::Cow;

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};

/// https://drafts.csswg.org/css-syntax-3/#decode-a-stylesheets-stream-of-bytes
/// returns the code points and the encoding that decoded them.
pub fn decode_stylesheet<'a>(
    bytes: &'a [u8],
    protocol_encoding_label: Option<&str>,
    environment_encoding: Option<&'static Encoding>,
) -> (Cow<'a, str>, &'static Encoding) {
    // step 1
    let fallback =
        determine_fallback_encoding(bytes, protocol_encoding_label, environment_encoding);
    // step 2: https://encoding.spec.whatwg.org/#decode, which lets a bom override the fallback.
    let (text, encoding, _) = fallback.decode(bytes);
    (text, encoding)
}

/// https://drafts.csswg.org/css-syntax-3/#determine-the-fallback-encoding
fn determine_fallback_encoding(
    bytes: &[u8],
    protocol_encoding_label: Option<&str>,
    environment_encoding: Option<&'static Encoding>,
) -> &'static Encoding {
    // step 1
    if let Some(encoding) =
        protocol_encoding_label.and_then(|label| Encoding::for_label(label.as_bytes()))
    {
        return encoding;
    }
    // step 2
    if let Some(encoding) = charset_label(bytes).and_then(Encoding::for_label) {
        return if encoding == UTF_16BE || encoding == UTF_16LE {
            UTF_8
        } else {
            encoding
        };
    }
    // step 3, 4
    environment_encoding.unwrap_or(UTF_8)
}

/// the XX bytes when the first 1024 bytes begin with `@charset "XX*";`, each XX being 0x00 to
/// 0x21 or 0x23 to 0x7f.
fn charset_label(bytes: &[u8]) -> Option<&[u8]> {
    let rest = bytes[..bytes.len().min(1024)].strip_prefix(b"@charset \"")?;
    let len = rest
        .iter()
        .position(|&b| !matches!(b, 0x00..=0x21 | 0x23..=0x7f))?;
    rest[len..].starts_with(b"\";").then_some(&rest[..len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charset_pattern_must_end_within_1024_bytes() {
        let mut bytes = b"@charset \"".to_vec();
        bytes.extend(std::iter::repeat_n(b'a', 1012));
        bytes.extend(b"\";");
        assert_eq!(charset_label(&bytes).map(<[u8]>::len), Some(1012));
        bytes.insert(10, b'a');
        assert_eq!(charset_label(&bytes), None);
        assert_eq!(charset_label(b"@charset \"x\";"), Some(&b"x"[..]));
    }
}
