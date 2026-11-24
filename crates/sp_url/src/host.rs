//! <https://url.spec.whatwg.org/#hosts-(domains-and-ip-addresses)>

use std::fmt;

use crate::idna::{domain_parser, is_forbidden_host_code_point};
use crate::percent::{EncodeSet, percent_decode_str, utf8_percent_encode};

/// <https://url.spec.whatwg.org/#concept-host>
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Host {
    /// a domain: a non-empty ascii string.
    Domain(String),
    Ipv4(u32),
    /// the eight 16-bit pieces.
    Ipv6([u16; 8]),
    /// an opaque host: a non-empty ascii string.
    Opaque(String),
    /// the empty host.
    Empty,
}

impl Host {
    /// the host parser: <https://url.spec.whatwg.org/#concept-host-parser>
    pub fn parse(input: &str, is_opaque: bool) -> Option<Host> {
        host_parse(input, is_opaque)
    }

    /// <https://url.spec.whatwg.org/#concept-host-serializer>
    pub fn serialize(&self) -> String {
        let mut output = String::new();
        self.serialize_into(&mut output);
        output
    }

    pub(crate) fn serialize_into(&self, output: &mut String) {
        match self {
            // step 1
            Host::Ipv4(address) => serialize_ipv4(*address, output),
            // step 2
            Host::Ipv6(address) => {
                output.push('[');
                serialize_ipv6(address, output);
                output.push(']');
            }
            // step 3
            Host::Domain(s) | Host::Opaque(s) => output.push_str(s),
            Host::Empty => {}
        }
    }
}

impl fmt::Display for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.serialize())
    }
}

/// <https://url.spec.whatwg.org/#concept-host-parser>
pub(crate) fn host_parse(input: &str, is_opaque: bool) -> Option<Host> {
    // step 1
    if let Some(rest) = input.strip_prefix('[') {
        // step 1.1
        let inner = rest.strip_suffix(']')?;
        // step 1.2
        return ipv6_parse(inner).map(Host::Ipv6);
    }
    // step 2
    if is_opaque {
        return opaque_host_parse(input);
    }
    // step 3: assert input is not the empty string. the host and file host
    // states never pass one for special urls, but don't rely on that.
    if input.is_empty() {
        return None;
    }
    // step 5
    let decoded = percent_decode_str(input);
    let domain = String::from_utf8_lossy(&decoded);
    // steps 6 and 7
    let ascii_domain = domain_parser(&domain, false)?;
    // step 8
    if ends_in_a_number(&ascii_domain) {
        return ipv4_parse(&ascii_domain).map(Host::Ipv4);
    }
    // step 9
    Some(Host::Domain(ascii_domain))
}

/// <https://url.spec.whatwg.org/#ends-in-a-number-checker>
fn ends_in_a_number(input: &str) -> bool {
    // step 2
    let mut parts: Vec<&str> = input.split('.').collect();
    // step 3
    if parts.last() == Some(&"") {
        if parts.len() == 1 {
            return false;
        }
        parts.pop();
    }
    // step 4
    let last = parts.last().copied().unwrap_or("");
    // step 5
    if !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // step 6
    parse_ipv4_number(last).is_some()
}

/// <https://url.spec.whatwg.org/#concept-ipv4-parser>
fn ipv4_parse(input: &str) -> Option<u32> {
    // step 1
    let mut parts: Vec<&str> = input.split('.').collect();
    // step 2
    if parts.last() == Some(&"") && parts.len() > 1 {
        parts.pop();
    }
    // step 4
    if parts.len() > 4 {
        return None;
    }
    // step 5 and 6
    let mut numbers = Vec::with_capacity(parts.len());
    for part in &parts {
        let (n, _) = parse_ipv4_number(part)?;
        numbers.push(n);
    }
    let (&last, rest) = numbers.split_last()?;
    // step 8
    if rest.iter().any(|&n| n > 255) {
        return None;
    }
    // step 9: 256^(5 - size) is at most 2^32, so it fits in a u64.
    if last >= 256u64.pow(5 - numbers.len() as u32) {
        return None;
    }
    // steps 10 to 13
    let mut ipv4 = last;
    for (counter, &n) in rest.iter().enumerate() {
        ipv4 += n * 256u64.pow(3 - counter as u32);
    }
    // step 14: the range checks above keep this below 2^32.
    u32::try_from(ipv4).ok()
}

/// <https://url.spec.whatwg.org/#ipv4-number-parser>
///
/// numbers too large for a u64 saturate, which every caller treats as out of
/// range anyway.
fn parse_ipv4_number(input: &str) -> Option<(u64, bool)> {
    // step 1
    if input.is_empty() {
        return None;
    }
    // steps 2 to 4
    let mut validation_error = false;
    let mut input = input;
    let mut radix = 10;
    if input.len() >= 2 && (input.starts_with("0x") || input.starts_with("0X")) {
        validation_error = true;
        input = &input[2..];
        radix = 16;
    } else if input.len() >= 2 && input.starts_with('0') {
        validation_error = true;
        input = &input[1..];
        radix = 8;
    }
    // step 5
    if input.is_empty() {
        return Some((0, true));
    }
    // steps 6 and 7
    let mut output: u64 = 0;
    for c in input.chars() {
        let digit = c.to_digit(radix)?;
        output = output
            .saturating_mul(u64::from(radix))
            .saturating_add(u64::from(digit));
    }
    // step 8
    Some((output, validation_error))
}

/// <https://url.spec.whatwg.org/#concept-ipv6-parser>
fn ipv6_parse(input: &str) -> Option<[u16; 8]> {
    let input: Vec<char> = input.chars().collect();
    let at = |p: usize| input.get(p).copied();
    // steps 1 to 4
    let mut address = [0u16; 8];
    let mut piece_index = 0usize;
    let mut compress: Option<usize> = None;
    let mut pointer = 0usize;
    // step 5
    if at(pointer) == Some(':') {
        // step 5.1
        if at(pointer + 1) != Some(':') {
            return None;
        }
        // step 5.2
        pointer += 2;
        // step 5.3
        piece_index += 1;
        compress = Some(piece_index);
    }
    // step 6
    while let Some(c) = at(pointer) {
        // step 6.1
        if piece_index == 8 {
            return None;
        }
        // step 6.2
        if c == ':' {
            // step 6.2.1
            if compress.is_some() {
                return None;
            }
            // step 6.2.2
            pointer += 1;
            piece_index += 1;
            compress = Some(piece_index);
            continue;
        }
        // step 6.3
        let mut value: u16 = 0;
        let mut length = 0;
        // step 6.4
        while length < 4
            && let Some(digit) = at(pointer).and_then(|c| c.to_digit(16))
        {
            value = value * 0x10 + digit as u16;
            pointer += 1;
            length += 1;
        }
        // step 6.5
        if at(pointer) == Some('.') {
            // step 6.5.1
            if length == 0 {
                return None;
            }
            // step 6.5.2
            pointer -= length;
            // step 6.5.3
            if piece_index > 6 {
                return None;
            }
            // step 6.5.4
            let mut numbers_seen = 0;
            // step 6.5.5
            while let Some(c) = at(pointer) {
                // step 6.5.5.1
                let mut ipv4_piece: Option<u16> = None;
                // step 6.5.5.2
                if numbers_seen > 0 {
                    if c == '.' && numbers_seen < 4 {
                        pointer += 1;
                    } else {
                        return None;
                    }
                }
                // step 6.5.5.3
                if !at(pointer).is_some_and(|c| c.is_ascii_digit()) {
                    return None;
                }
                // step 6.5.5.4
                while let Some(number) = at(pointer).and_then(|c| c.to_digit(10)) {
                    let number = number as u16;
                    ipv4_piece = match ipv4_piece {
                        None => Some(number),
                        Some(0) => return None,
                        Some(piece) => Some(piece * 10 + number),
                    };
                    if ipv4_piece > Some(255) {
                        return None;
                    }
                    pointer += 1;
                }
                // step 6.5.5.5
                let piece = ipv4_piece.unwrap_or(0);
                address[piece_index] = address[piece_index] * 0x100 + piece;
                // step 6.5.5.6
                numbers_seen += 1;
                // step 6.5.5.7
                if numbers_seen == 2 || numbers_seen == 4 {
                    piece_index += 1;
                }
            }
            // step 6.5.6
            if numbers_seen != 4 {
                return None;
            }
            // step 6.5.7
            break;
        } else if at(pointer) == Some(':') {
            // step 6.6
            pointer += 1;
            // a trailing u+003a (:) is a failure.
            at(pointer)?;
        } else if at(pointer).is_some() {
            // step 6.7
            return None;
        }
        // step 6.9
        address[piece_index] = value;
        // step 6.10
        piece_index += 1;
    }
    // step 7
    if let Some(compress) = compress {
        // step 7.1
        let mut swaps = piece_index - compress;
        // step 7.2
        piece_index = 7;
        // step 7.3
        while piece_index != 0 && swaps > 0 {
            address.swap(piece_index, compress + swaps - 1);
            piece_index -= 1;
            swaps -= 1;
        }
    } else if piece_index != 8 {
        // step 8
        return None;
    }
    // step 9
    Some(address)
}

/// <https://url.spec.whatwg.org/#concept-opaque-host-parser>
fn opaque_host_parse(input: &str) -> Option<Host> {
    // step 1
    if input.chars().any(is_forbidden_host_code_point) {
        return None;
    }
    // step 4
    let encoded = utf8_percent_encode(input, EncodeSet::C0Control);
    if encoded.is_empty() {
        Some(Host::Empty)
    } else {
        Some(Host::Opaque(encoded))
    }
}

/// <https://url.spec.whatwg.org/#concept-ipv4-serializer>
fn serialize_ipv4(address: u32, output: &mut String) {
    let [a, b, c, d] = address.to_be_bytes();
    output.push_str(&format!("{a}.{b}.{c}.{d}"));
}

/// <https://url.spec.whatwg.org/#concept-ipv6-serializer>
fn serialize_ipv6(address: &[u16; 8], output: &mut String) {
    // step 2
    let compress = compressed_piece_index(address);
    // step 3
    let mut ignore0 = false;
    // step 4
    for (piece_index, &piece) in address.iter().enumerate() {
        // step 4.1
        if ignore0 && piece == 0 {
            continue;
        }
        // step 4.2
        ignore0 = false;
        // step 4.3
        if compress == Some(piece_index) {
            output.push_str(if piece_index == 0 { "::" } else { ":" });
            ignore0 = true;
            continue;
        }
        // step 4.4
        output.push_str(&format!("{piece:x}"));
        // step 4.5
        if piece_index != 7 {
            output.push(':');
        }
    }
}

/// <https://url.spec.whatwg.org/#find-the-ipv6-address-compressed-piece-index>
fn compressed_piece_index(address: &[u16; 8]) -> Option<usize> {
    // steps 1 to 4
    let mut longest_index = None;
    let mut longest_size = 1;
    let mut found_index = None;
    let mut found_size = 0;
    // step 5
    for (piece_index, &piece) in address.iter().enumerate() {
        if piece != 0 {
            // step 5.1
            if found_size > longest_size {
                longest_index = found_index;
                longest_size = found_size;
            }
            found_index = None;
            found_size = 0;
        } else {
            // step 5.2
            if found_index.is_none() {
                found_index = Some(piece_index);
            }
            found_size += 1;
        }
    }
    // step 6
    if found_size > longest_size {
        return found_index;
    }
    // step 7
    longest_index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Option<String> {
        host_parse(s, false).map(|h| h.serialize())
    }

    #[test]
    fn ipv4() {
        assert_eq!(parse("192.168.0.1").as_deref(), Some("192.168.0.1"));
        assert_eq!(parse("0x7f.1").as_deref(), Some("127.0.0.1"));
        assert_eq!(parse("0300.0250.0.1").as_deref(), Some("192.168.0.1"));
        assert_eq!(parse("4294967295").as_deref(), Some("255.255.255.255"));
        assert_eq!(parse("4294967296"), None);
        assert_eq!(parse("1.2.3.4.5"), None);
        assert_eq!(parse("1.2.3.4."), Some("1.2.3.4".to_owned()));
        assert_eq!(parse("256.1.1.1"), None);
        assert_eq!(parse("09"), None);
        assert_eq!(parse("0x"), Some("0.0.0.0".to_owned()));
        assert_eq!(parse(&format!("0x{}", "f".repeat(100))), None);
        assert_eq!(parse("1.2.3.foo").as_deref(), Some("1.2.3.foo"));
        assert_eq!(parse("foo.0x"), None);
    }

    #[test]
    fn ipv6() {
        assert_eq!(parse("[::]").as_deref(), Some("[::]"));
        assert_eq!(parse("[::1]").as_deref(), Some("[::1]"));
        assert_eq!(parse("[1:0:0:2::3:0]").as_deref(), Some("[1::2:0:0:3:0]"));
        assert_eq!(
            parse("[0:f:0:0:f:f:0:0]").as_deref(),
            Some("[0:f::f:f:0:0]")
        );
        assert_eq!(parse("[::127.0.0.1]").as_deref(), Some("[::7f00:1]"));
        assert_eq!(parse("[1:2:3:4:5:6:7:8:9]"), None);
        assert_eq!(parse("[::1.2.3.]"), None);
        assert_eq!(parse("[::1.2.3.04]"), None);
        assert_eq!(parse("[1:]"), None);
        assert_eq!(parse("[:1]"), None);
        assert_eq!(parse("[1::2::3]"), None);
        assert_eq!(parse("[1"), None);
        assert_eq!(parse("[12345::]"), None);
    }

    #[test]
    fn opaque() {
        assert_eq!(
            host_parse("a\u{e9}%zz", true),
            Some(Host::Opaque("a%C3%A9%zz".to_owned()))
        );
        assert_eq!(host_parse("a b", true), None);
        assert_eq!(host_parse("", true), Some(Host::Empty));
    }

    #[test]
    fn domain() {
        assert_eq!(parse("ExAmPle.CoM").as_deref(), Some("example.com"));
        assert_eq!(parse("%41.com").as_deref(), Some("a.com"));
        assert_eq!(parse("%zz"), None);
        assert_eq!(parse("a%80"), None);
    }
}
