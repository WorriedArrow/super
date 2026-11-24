//! <https://url.spec.whatwg.org/#application/x-www-form-urlencoded>

use crate::percent::{EncodeSet, percent_decode, utf8_percent_encode_into};

/// <https://url.spec.whatwg.org/#concept-urlencoded-parser>
pub fn parse(input: &[u8]) -> Vec<(String, String)> {
    // steps 1 and 2
    let mut output = Vec::new();
    // step 3
    for bytes in input.split(|&b| b == b'&') {
        // step 3.1
        if bytes.is_empty() {
            continue;
        }
        // step 3.2 and 3.3
        let (name, value) = match bytes.iter().position(|&b| b == b'=') {
            Some(i) => (&bytes[..i], &bytes[i + 1..]),
            None => (bytes, &[][..]),
        };
        // steps 3.4 and 3.5
        let decode = |b: &[u8]| {
            let replaced: Vec<u8> = b
                .iter()
                .map(|&b| if b == b'+' { b' ' } else { b })
                .collect();
            String::from_utf8_lossy(&percent_decode(&replaced)).into_owned()
        };
        // step 3.6
        output.push((decode(name), decode(value)));
    }
    // step 4
    output
}

/// <https://url.spec.whatwg.org/#concept-urlencoded-string-parser>
pub fn parse_str(input: &str) -> Vec<(String, String)> {
    parse(input.as_bytes())
}

/// <https://url.spec.whatwg.org/#concept-urlencoded-serializer>, with utf-8
/// as the encoding.
pub fn serialize<N: AsRef<str>, V: AsRef<str>>(tuples: &[(N, V)]) -> String {
    // step 2
    let mut output = String::new();
    // step 3
    for (name, value) in tuples {
        // step 3.4
        if !output.is_empty() {
            output.push('&');
        }
        // steps 3.2, 3.3 and 3.5
        utf8_percent_encode_into(name.as_ref(), EncodeSet::FormUrlencoded, &mut output);
        output.push('=');
        utf8_percent_encode_into(value.as_ref(), EncodeSet::FormUrlencoded, &mut output);
    }
    // step 4
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let parsed = parse_str("a=b&&c&=d&e=f=g&h+i=%26%zz&%FF=x");
        assert_eq!(
            parsed,
            [
                ("a", "b"),
                ("c", ""),
                ("", "d"),
                ("e", "f=g"),
                ("h i", "&%zz"),
                ("\u{fffd}", "x"),
            ]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
        );
        assert_eq!(
            serialize(&parsed),
            "a=b&c=&=d&e=f%3Dg&h+i=%26%25zz&%EF%BF%BD=x"
        );
    }
}
