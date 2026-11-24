//! <https://url.spec.whatwg.org/#percent-encoded-bytes>

/// the percent-encode sets: <https://url.spec.whatwg.org/#percent-encode-set>
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeSet {
    C0Control,
    Fragment,
    Query,
    SpecialQuery,
    Path,
    Userinfo,
    Component,
    FormUrlencoded,
}

impl EncodeSet {
    /// whether `c` is in this percent-encode set.
    pub fn contains(self, c: char) -> bool {
        // https://url.spec.whatwg.org/#c0-control-percent-encode-set
        if c <= '\u{1f}' || c > '~' {
            return true;
        }
        match self {
            EncodeSet::C0Control => false,
            EncodeSet::Fragment => matches!(c, ' ' | '"' | '<' | '>' | '`'),
            EncodeSet::Query => is_query(c),
            EncodeSet::SpecialQuery => is_query(c) || c == '\'',
            EncodeSet::Path => is_path(c),
            EncodeSet::Userinfo => is_userinfo(c),
            EncodeSet::Component => is_component(c),
            EncodeSet::FormUrlencoded => {
                is_component(c) || matches!(c, '!' | '\'' | '(' | ')' | '~')
            }
        }
    }
}

fn is_query(c: char) -> bool {
    matches!(c, ' ' | '"' | '#' | '<' | '>')
}

fn is_path(c: char) -> bool {
    is_query(c) || matches!(c, '?' | '^' | '`' | '{' | '}')
}

fn is_userinfo(c: char) -> bool {
    is_path(c) || matches!(c, '/' | ':' | ';' | '=' | '@' | '[' | '\\' | ']' | '|')
}

fn is_component(c: char) -> bool {
    is_userinfo(c) || matches!(c, '$' | '%' | '&' | '+' | ',')
}

const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";

/// <https://url.spec.whatwg.org/#percent-encode>
pub fn percent_encode_byte(byte: u8, output: &mut String) {
    output.push('%');
    output.push(UPPER_HEX[usize::from(byte >> 4)] as char);
    output.push(UPPER_HEX[usize::from(byte & 0xf)] as char);
}

/// <https://url.spec.whatwg.org/#string-percent-encode-after-encoding> with
/// utf-8 as the encoding.
pub fn utf8_percent_encode_into(input: &str, set: EncodeSet, output: &mut String) {
    // step 2
    let space_as_plus = set == EncodeSet::FormUrlencoded;
    let mut buf = [0u8; 4];
    for c in input.chars() {
        // step 6.2.1
        if space_as_plus && c == ' ' {
            output.push('+');
            continue;
        }
        // steps 6.2.2 to 6.2.5: every non-ascii code point is in every
        // percent-encode set, so ascii code points map to themselves.
        if !set.contains(c) {
            output.push(c);
            continue;
        }
        for &byte in c.encode_utf8(&mut buf).as_bytes() {
            percent_encode_byte(byte, output);
        }
    }
}

/// <https://url.spec.whatwg.org/#string-utf-8-percent-encode>
pub fn utf8_percent_encode(input: &str, set: EncodeSet) -> String {
    let mut output = String::with_capacity(input.len());
    utf8_percent_encode_into(input, set, &mut output);
    output
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// <https://url.spec.whatwg.org/#percent-decode>
pub fn percent_decode(input: &[u8]) -> Vec<u8> {
    // step 1
    let mut output = Vec::with_capacity(input.len());
    // step 2
    let mut i = 0;
    while i < input.len() {
        let byte = input[i];
        // step 2.1
        if byte != b'%' {
            output.push(byte);
            i += 1;
            continue;
        }
        // steps 2.2 and 2.3
        match (
            input.get(i + 1).copied().and_then(hex_value),
            input.get(i + 2).copied().and_then(hex_value),
        ) {
            (Some(high), Some(low)) => {
                output.push(high << 4 | low);
                i += 3;
            }
            _ => {
                output.push(byte);
                i += 1;
            }
        }
    }
    output
}

/// <https://url.spec.whatwg.org/#string-percent-decode>
pub fn percent_decode_str(input: &str) -> Vec<u8> {
    percent_decode(input.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_byte() {
        let mut s = String::new();
        percent_encode_byte(0x23, &mut s);
        percent_encode_byte(0x7f, &mut s);
        assert_eq!(s, "%23%7F");
    }

    #[test]
    fn decode() {
        assert_eq!(percent_decode(b"%25%s%1G"), b"%%s%1G");
        assert_eq!(
            percent_decode_str("\u{203d}%25%2E"),
            [0xe2, 0x80, 0xbd, 0x25, 0x2e]
        );
        assert_eq!(percent_decode(b"%"), b"%");
        assert_eq!(percent_decode(b"%4"), b"%4");
        assert_eq!(percent_decode(b"%41%"), b"A%");
    }

    #[test]
    fn userinfo() {
        assert_eq!(
            utf8_percent_encode("\u{2261}", EncodeSet::Userinfo),
            "%E2%89%A1"
        );
        assert_eq!(
            utf8_percent_encode("Say what\u{203d}", EncodeSet::Userinfo),
            "Say%20what%E2%80%BD"
        );
    }

    #[test]
    fn form_urlencoded() {
        assert_eq!(
            utf8_percent_encode("1+1 = 2%20*", EncodeSet::FormUrlencoded),
            "1%2B1+%3D+2%2520*"
        );
    }

    #[test]
    fn component_matches_encode_uri_component() {
        // encodeURIComponent leaves A-Z a-z 0-9 - _ . ! ~ * ' ( ) alone.
        for b in 0x20u8..0x7f {
            let c = b as char;
            let unreserved = c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c);
            assert_eq!(EncodeSet::Component.contains(c), !unreserved, "{c:?}");
        }
    }
}
