//! idna processing for hosts: <https://url.spec.whatwg.org/#idna> and
//! <https://www.unicode.org/reports/tr46/>

use icu_normalizer::uts46::Uts46MapperBorrowed;
use icu_properties::CodePointMapData;
use icu_properties::props::{BidiClass, GeneralCategory, GeneralCategoryGroup, JoiningType};

use crate::punycode;

/// the uts #46 options the url standard varies. CheckBidi and CheckJoiners
/// are always true, Transitional_Processing and IgnoreInvalidPunycode are
/// always false.
#[derive(Clone, Copy)]
struct Options {
    check_hyphens: bool,
    use_std3_ascii_rules: bool,
    verify_dns_length: bool,
}

/// <https://url.spec.whatwg.org/#concept-domain-parser>
pub(crate) fn domain_parser(domain: &str, be_strict: bool) -> Option<String> {
    // steps 1 and 2: the strict run only reports a validation error unless
    // beStrict is true, and validation errors aren't surfaced.
    if be_strict {
        return domain_parser_to_ascii(domain, true);
    }
    // step 4
    let result = if domain.is_ascii() {
        domain.to_ascii_lowercase()
    } else {
        domain_parser_to_ascii(domain, false)?
    };
    // step 5
    if result.is_empty() {
        return None;
    }
    // step 6
    if result.chars().any(is_forbidden_domain_code_point) {
        return None;
    }
    // step 7
    Some(result)
}

/// <https://url.spec.whatwg.org/#concept-domain-parser-to-ascii>
fn domain_parser_to_ascii(domain: &str, be_strict: bool) -> Option<String> {
    unicode_to_ascii(
        domain,
        Options {
            check_hyphens: be_strict,
            use_std3_ascii_rules: be_strict,
            verify_dns_length: be_strict,
        },
    )
}

/// <https://url.spec.whatwg.org/#concept-domain-to-unicode>
pub fn domain_to_unicode(domain: &str) -> String {
    // step 1
    let (labels, error) = processing(
        domain,
        Options {
            check_hyphens: false,
            use_std3_ascii_rules: false,
            verify_dns_length: false,
        },
    );
    // step 2
    if error {
        return domain.to_owned();
    }
    // step 3
    labels.join(".")
}

/// <https://url.spec.whatwg.org/#forbidden-host-code-point>
pub(crate) fn is_forbidden_host_code_point(c: char) -> bool {
    matches!(
        c,
        '\0' | '\t'
            | '\n'
            | '\r'
            | ' '
            | '#'
            | '/'
            | ':'
            | '<'
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '|'
    )
}

/// <https://url.spec.whatwg.org/#forbidden-domain-code-point>
pub(crate) fn is_forbidden_domain_code_point(c: char) -> bool {
    is_forbidden_host_code_point(c) || c <= '\u{1f}' || c == '%' || c == '\u{7f}'
}

/// ToASCII: <https://www.unicode.org/reports/tr46/#ToASCII>
fn unicode_to_ascii(domain: &str, options: Options) -> Option<String> {
    // step 1
    let (labels, mut error) = processing(domain, options);
    // steps 2 and 3
    let mut ascii_labels = Vec::with_capacity(labels.len());
    for label in labels {
        if label.is_ascii() {
            ascii_labels.push(label);
        } else if let Some(encoded) = punycode::encode(&label) {
            ascii_labels.push(format!("xn--{encoded}"));
        } else {
            error = true;
            ascii_labels.push(label);
        }
    }
    // step 4
    if options.verify_dns_length {
        let mut labels: &[String] = &ascii_labels;
        // the root label and its dot don't count.
        if labels.len() > 1 && labels.last().is_some_and(|l| l.is_empty()) {
            labels = &labels[..labels.len() - 1];
        }
        let total = labels.iter().map(|l| l.len()).sum::<usize>() + labels.len() - 1;
        if !(1..=253).contains(&total) || labels.iter().any(|l| !(1..=63).contains(&l.len())) {
            error = true;
        }
    }
    // step 5
    if error {
        return None;
    }
    Some(ascii_labels.join("."))
}

/// Processing: <https://www.unicode.org/reports/tr46/#Processing>
///
/// returns the processed labels and whether an error was recorded.
fn processing(domain: &str, options: Options) -> (Vec<String>, bool) {
    let mapper = Uts46MapperBorrowed::new();
    let mut error = false;
    // steps 1 and 2: map, then normalize to nfc. disallowed code points come
    // out as u+fffd, which is itself disallowed.
    let mapped: String = mapper.map_normalize(domain.chars()).collect();
    if mapped.contains('\u{fffd}') {
        error = true;
    }
    // step 3
    let mut labels: Vec<String> = mapped.split('.').map(str::to_owned).collect();
    // step 4
    for label in &mut labels {
        // step 4.1
        if let Some(rest) = label.strip_prefix("xn--") {
            // step 4.1.1
            if !rest.is_ascii() {
                error = true;
                continue;
            }
            // step 4.1.2
            let Some(decoded) = punycode::decode(rest) else {
                error = true;
                continue;
            };
            // step 4.1.3
            if decoded.is_empty() || decoded.is_ascii() {
                error = true;
            }
            // step 4.1.4
            if !is_valid_label(&mapper, &decoded, options, true) {
                error = true;
            }
            // step 4.1.5
            *label = decoded;
        } else if !is_valid_label(&mapper, label, options, false) {
            // step 4.2
            error = true;
        }
    }
    // validity criterion 9 depends on every label, so it runs once all labels
    // are converted.
    if is_bidi_domain_name(&labels)
        && labels
            .iter()
            .any(|label| !label.is_empty() && !satisfies_bidi_rule(label))
    {
        error = true;
    }
    // step 5
    (labels, error)
}

/// Validity Criteria, criteria 1 to 8:
/// <https://www.unicode.org/reports/tr46/#Validity_Criteria>
fn is_valid_label(
    mapper: &Uts46MapperBorrowed<'_>,
    label: &str,
    options: Options,
    from_punycode: bool,
) -> bool {
    // criteria 1 and 7: labels that didn't come from punycode were mapped
    // and normalized in processing steps 1 and 2, and any disallowed code
    // point already recorded an error. a decoded label must come back
    // unchanged from the nfc and status checks.
    if from_punycode && !mapper.normalize_validate(label.chars()).eq(label.chars()) {
        return false;
    }
    let bytes = label.as_bytes();
    if options.check_hyphens {
        // criterion 2
        if bytes.len() >= 4 && bytes[2] == b'-' && bytes[3] == b'-' {
            return false;
        }
        // criterion 3
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
    } else if label.starts_with("xn--") {
        // criterion 4
        return false;
    }
    // criterion 5
    if label.contains('.') {
        return false;
    }
    // criterion 6
    if let Some(first) = label.chars().next()
        && GeneralCategoryGroup::Mark
            .contains(CodePointMapData::<GeneralCategory>::new().get(first))
    {
        return false;
    }
    // criterion 7, UseSTD3ASCIIRules part
    if options.use_std3_ascii_rules
        && label
            .chars()
            .any(|c| c.is_ascii() && !matches!(c, 'a'..='z' | '0'..='9' | '-'))
    {
        return false;
    }
    // criterion 8
    satisfies_context_j(mapper, label)
}

/// the ContextJ rules: <https://www.rfc-editor.org/rfc/rfc5892#appendix-A.1>
/// and <https://www.rfc-editor.org/rfc/rfc5892#appendix-A.2>
fn satisfies_context_j(mapper: &Uts46MapperBorrowed<'_>, label: &str) -> bool {
    if !label.contains(['\u{200c}', '\u{200d}']) {
        return true;
    }
    let chars: Vec<char> = label.chars().collect();
    let joining_type = CodePointMapData::<JoiningType>::new();
    for (i, &c) in chars.iter().enumerate() {
        if c != '\u{200c}' && c != '\u{200d}' {
            continue;
        }
        // both rules: if Canonical_Combining_Class(Before(cp)) .eq. Virama
        // then True.
        if i > 0 && mapper.is_virama(chars[i - 1]) {
            continue;
        }
        // a.2 (zero width joiner): otherwise False.
        if c == '\u{200d}' {
            return false;
        }
        // a.1 (zero width non-joiner): RegExpMatch((Joining_Type:{L,D})
        // (Joining_Type:T)* zwnj (Joining_Type:T)*(Joining_Type:{R,D})).
        let left = chars[..i]
            .iter()
            .rev()
            .map(|&c| joining_type.get(c))
            .find(|&jt| jt != JoiningType::Transparent);
        let right = chars[i + 1..]
            .iter()
            .map(|&c| joining_type.get(c))
            .find(|&jt| jt != JoiningType::Transparent);
        let left_ok = matches!(
            left,
            Some(JoiningType::LeftJoining | JoiningType::DualJoining)
        );
        let right_ok = matches!(
            right,
            Some(JoiningType::RightJoining | JoiningType::DualJoining)
        );
        if !(left_ok && right_ok) {
            return false;
        }
    }
    true
}

/// a bidi domain name: <https://www.rfc-editor.org/rfc/rfc5893#section-1.4>
fn is_bidi_domain_name(labels: &[String]) -> bool {
    let bidi = CodePointMapData::<BidiClass>::new();
    labels.iter().flat_map(|l| l.chars()).any(|c| {
        matches!(
            bidi.get(c),
            BidiClass::RightToLeft | BidiClass::ArabicLetter | BidiClass::ArabicNumber
        )
    })
}

/// the bidi rule: <https://www.rfc-editor.org/rfc/rfc5893#section-2>
fn satisfies_bidi_rule(label: &str) -> bool {
    use BidiClass as B;
    let bidi = CodePointMapData::<BidiClass>::new();
    let mut classes = label.chars().map(|c| bidi.get(c));
    let Some(first) = classes.next() else {
        return true;
    };
    let classes: Vec<BidiClass> = std::iter::once(first).chain(classes).collect();
    // the class of the last character that isn't nsm.
    let last = classes
        .iter()
        .rev()
        .copied()
        .find(|&c| c != B::NonspacingMark);
    match first {
        // rule 1: an rtl label.
        B::RightToLeft | B::ArabicLetter => {
            // rule 2
            let allowed = classes.iter().all(|c| {
                matches!(
                    *c,
                    B::RightToLeft
                        | B::ArabicLetter
                        | B::ArabicNumber
                        | B::EuropeanNumber
                        | B::EuropeanSeparator
                        | B::CommonSeparator
                        | B::EuropeanTerminator
                        | B::OtherNeutral
                        | B::BoundaryNeutral
                        | B::NonspacingMark
                )
            });
            // rule 3
            let end_ok = matches!(
                last,
                Some(B::RightToLeft | B::ArabicLetter | B::EuropeanNumber | B::ArabicNumber)
            );
            // rule 4
            let numbers_ok =
                !(classes.contains(&B::EuropeanNumber) && classes.contains(&B::ArabicNumber));
            allowed && end_ok && numbers_ok
        }
        // rule 1: an ltr label.
        B::LeftToRight => {
            // rule 5
            let allowed = classes.iter().all(|c| {
                matches!(
                    *c,
                    B::LeftToRight
                        | B::EuropeanNumber
                        | B::EuropeanSeparator
                        | B::CommonSeparator
                        | B::EuropeanTerminator
                        | B::OtherNeutral
                        | B::BoundaryNeutral
                        | B::NonspacingMark
                )
            });
            // rule 6
            let end_ok = matches!(last, Some(B::LeftToRight | B::EuropeanNumber));
            allowed && end_ok
        }
        // rule 1 fails.
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_is_lowercased_without_validation() {
        assert_eq!(
            domain_parser("EXAMPLE.com", false).as_deref(),
            Some("example.com")
        );
        assert_eq!(domain_parser("xn--a", false).as_deref(), Some("xn--a"));
        assert_eq!(domain_parser("-x", false).as_deref(), Some("-x"));
    }

    #[test]
    fn unicode() {
        assert_eq!(
            domain_parser("fa\u{df}.de", false).as_deref(),
            Some("xn--fa-hia.de")
        );
        assert_eq!(
            domain_parser("\u{2615}.example", false).as_deref(),
            Some("xn--53h.example")
        );
        assert_eq!(domain_parser("xn--a.\u{df}", false), None);
        assert_eq!(domain_parser("a\u{fffd}", false), None);
    }

    #[test]
    fn forbidden_and_empty() {
        assert_eq!(domain_parser("", false), None);
        assert_eq!(domain_parser("a%b", false), None);
        assert_eq!(domain_parser("a\u{ad}", false).as_deref(), Some("a"));
        // only ignored code points map to the empty string.
        assert_eq!(domain_parser("\u{ad}", false), None);
    }

    #[test]
    fn strict() {
        assert_eq!(domain_parser("-x", true), None);
        assert_eq!(domain_parser("a_b", true), None);
        assert_eq!(
            domain_parser("example.com.", true).as_deref(),
            Some("example.com.")
        );
        let long = "a".repeat(64);
        assert_eq!(domain_parser(&long, true), None);
    }

    #[test]
    fn bidi() {
        // rule 1: an rtl domain whose other label starts with a digit.
        assert_eq!(domain_parser("0a.\u{5d0}", false), None);
        assert!(domain_parser("a.\u{5d0}", false).is_some());
        // rule 4: en and an mixed in an rtl label.
        assert_eq!(domain_parser("\u{5d0}1\u{660}", false), None);
    }

    #[test]
    fn joiners() {
        // zwj after a virama (devanagari ka, virama).
        assert!(domain_parser("\u{915}\u{94d}\u{200d}", false).is_some());
        assert_eq!(domain_parser("a\u{200d}b", false), None);
        // zwnj between two dual-joining arabic letters.
        assert!(domain_parser("\u{628}\u{200c}\u{628}", false).is_some());
        assert_eq!(domain_parser("a\u{200c}b", false), None);
    }

    #[test]
    fn to_unicode() {
        assert_eq!(domain_to_unicode("xn--fa-hia.de"), "fa\u{df}.de");
        assert_eq!(domain_to_unicode("xn--8i7caa"), "xn--8i7caa");
        assert_eq!(domain_to_unicode("example.com"), "example.com");
    }
}
