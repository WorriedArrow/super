//! punycode: <https://www.rfc-editor.org/rfc/rfc3492>

// https://www.rfc-editor.org/rfc/rfc3492#section-5
const BASE: u32 = 36;
const TMIN: u32 = 1;
const TMAX: u32 = 26;
const SKEW: u32 = 38;
const DAMP: u32 = 700;
const INITIAL_BIAS: u32 = 72;
const INITIAL_N: u32 = 128;

/// <https://www.rfc-editor.org/rfc/rfc3492#section-6.1>
fn adapt(delta: u32, numpoints: u32, firsttime: bool) -> u32 {
    let mut delta = if firsttime { delta / DAMP } else { delta / 2 };
    delta += delta / numpoints;
    let mut k = 0;
    while delta > ((BASE - TMIN) * TMAX) / 2 {
        delta /= BASE - TMIN;
        k += BASE;
    }
    k + (((BASE - TMIN + 1) * delta) / (delta + SKEW))
}

fn threshold(k: u32, bias: u32) -> u32 {
    if k <= bias {
        TMIN
    } else if k >= bias + TMAX {
        TMAX
    } else {
        k - bias
    }
}

fn decode_digit(byte: u8) -> Option<u32> {
    match byte {
        b'a'..=b'z' => Some(u32::from(byte - b'a')),
        b'A'..=b'Z' => Some(u32::from(byte - b'A')),
        b'0'..=b'9' => Some(u32::from(byte - b'0') + 26),
        _ => None,
    }
}

fn encode_digit(d: u32) -> char {
    // 0..25 map to a..z, 26..35 map to 0..9.
    match d {
        0..=25 => char::from(b'a' + d as u8),
        _ => char::from(b'0' + (d - 26) as u8),
    }
}

/// the decoding procedure: <https://www.rfc-editor.org/rfc/rfc3492#section-6.2>
pub(crate) fn decode(input: &str) -> Option<String> {
    if !input.is_ascii() {
        return None;
    }
    let mut n = INITIAL_N;
    let mut i: u32 = 0;
    let mut bias = INITIAL_BIAS;
    // copy the basic code points before the last delimiter, and consume
    // the delimiter itself.
    let (basic, extended) = match input.rfind('-') {
        Some(b) => (&input[..b], &input[b + 1..]),
        None => ("", input),
    };
    let mut output: Vec<char> = basic.chars().collect();
    let extended = extended.as_bytes();
    let mut pos = 0;
    while pos < extended.len() {
        let oldi = i;
        let mut w: u32 = 1;
        let mut k = BASE;
        loop {
            // fail if there's no more input.
            let digit = decode_digit(*extended.get(pos)?)?;
            pos += 1;
            i = i.checked_add(digit.checked_mul(w)?)?;
            let t = threshold(k, bias);
            if digit < t {
                break;
            }
            w = w.checked_mul(BASE - t)?;
            k += BASE;
        }
        let len = u32::try_from(output.len()).ok()?.checked_add(1)?;
        bias = adapt(i - oldi, len, oldi == 0);
        n = n.checked_add(i / len)?;
        i %= len;
        // n starts at 128 and only grows, so it is never a basic code point.
        // surrogates and values past u+10ffff aren't scalar values.
        let c = char::from_u32(n)?;
        output.insert(i as usize, c);
        i += 1;
    }
    Some(output.into_iter().collect())
}

/// the encoding procedure: <https://www.rfc-editor.org/rfc/rfc3492#section-6.3>
pub(crate) fn encode(input: &str) -> Option<String> {
    let input: Vec<u32> = input.chars().map(u32::from).collect();
    let mut n = INITIAL_N;
    let mut delta: u32 = 0;
    let mut bias = INITIAL_BIAS;
    let mut output: String = input
        .iter()
        .filter(|&&c| c < 0x80)
        .map(|&c| char::from(c as u8))
        .collect();
    let b = u32::try_from(output.len()).ok()?;
    let mut h = b;
    if b > 0 {
        output.push('-');
    }
    let total = u32::try_from(input.len()).ok()?;
    while h < total {
        let m = input.iter().copied().filter(|&c| c >= n).min()?;
        delta = delta.checked_add((m - n).checked_mul(h + 1)?)?;
        n = m;
        for &c in &input {
            if c < n {
                delta = delta.checked_add(1)?;
            }
            if c == n {
                let mut q = delta;
                let mut k = BASE;
                loop {
                    let t = threshold(k, bias);
                    if q < t {
                        break;
                    }
                    output.push(encode_digit(t + (q - t) % (BASE - t)));
                    q = (q - t) / (BASE - t);
                    k += BASE;
                }
                output.push(encode_digit(q));
                bias = adapt(delta, h + 1, h == b);
                delta = 0;
                h += 1;
            }
        }
        delta = delta.checked_add(1)?;
        n = n.checked_add(1)?;
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // https://www.rfc-editor.org/rfc/rfc3492#section-7.1
    const SAMPLES: &[(&str, &str)] = &[
        (
            "\u{644}\u{64a}\u{647}\u{645}\u{627}\u{628}\u{62a}\u{643}\u{644}\u{645}\u{648}\u{634}\u{639}\u{631}\u{628}\u{64a}\u{61f}",
            "egbpdaj6bu4bxfgehfvwxn",
        ),
        (
            "\u{4ed6}\u{4eec}\u{4e3a}\u{4ec0}\u{4e48}\u{4e0d}\u{8bf4}\u{4e2d}\u{6587}",
            "ihqwcrb4cv8a8dqg056pqjye",
        ),
        (
            "3\u{5e74}B\u{7d44}\u{91d1}\u{516b}\u{5148}\u{751f}",
            "3B-ww4c5e180e575a65lsy2b",
        ),
        ("-> $1.00 <-", "-> $1.00 <--"),
        ("b\u{fc}cher", "bcher-kva"),
    ];

    #[test]
    fn rfc_samples() {
        for (unicode, puny) in SAMPLES {
            assert_eq!(encode(unicode).as_deref(), Some(*puny), "{unicode}");
            assert_eq!(decode(puny).as_deref(), Some(*unicode), "{puny}");
        }
    }

    #[test]
    fn invalid() {
        // a digit at or above the threshold needs another digit after it.
        assert_eq!(decode("b"), None);
        assert_eq!(decode("a").as_deref(), Some("\u{80}"));
        assert_eq!(decode("99999999999"), None);
        assert_eq!(decode("\u{e9}"), None);
        assert_eq!(decode("ab!"), None);
        assert_eq!(decode(""), Some(String::new()));
    }
}
