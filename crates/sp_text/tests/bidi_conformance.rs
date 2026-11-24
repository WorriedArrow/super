//! uax #9 conformance: every case of BidiTest.txt and BidiCharacterTest.txt
//! from unicode 18.0.0, the version of the icu_properties data sp_text uses.
//! https://www.unicode.org/reports/tr41/#Bidi9 describes both formats.

use sp_text::bidi::{BidiClass, Level, line_levels, reorder_visual, resolve_levels};

const BIDI_TEST: &str = include_str!("data/BidiTest.txt");
const BIDI_CHARACTER_TEST: &str = include_str!("data/BidiCharacterTest.txt");

/// a character with each bidi class. BidiTest.txt assumes none of them is a
/// paired bracket, so `!` stands for on.
fn sample(class: &str) -> char {
    match class {
        "L" => 'a',
        "R" => '\u{5d0}',
        "AL" => '\u{627}',
        "EN" => '1',
        "ES" => '+',
        "ET" => '#',
        "AN" => '\u{660}',
        "CS" => ',',
        "NSM" => '\u{300}',
        "BN" => '\u{ad}',
        "B" => '\u{2029}',
        "S" => '\t',
        "WS" => ' ',
        "ON" => '!',
        "LRE" => '\u{202a}',
        "LRO" => '\u{202d}',
        "RLE" => '\u{202b}',
        "RLO" => '\u{202e}',
        "PDF" => '\u{202c}',
        "LRI" => '\u{2066}',
        "RLI" => '\u{2067}',
        "FSI" => '\u{2068}',
        "PDI" => '\u{2069}',
        other => panic!("unknown bidi class {other}"),
    }
}

/// levels after l1 per character, and the visual order of the characters
/// that x9 doesn't remove (the ones whose expected level isn't `x`).
fn run_case(text: &str, base: Option<Level>, keep: &[bool]) -> (Level, Vec<u8>, Vec<usize>) {
    let para = resolve_levels(text, base);
    let line = line_levels(text, &para, 0..text.len());
    let levels: Vec<Level> = text.char_indices().map(|(i, _)| line[i]).collect();
    let kept: Vec<usize> = (0..levels.len()).filter(|&i| keep[i]).collect();
    let kept_levels: Vec<Level> = kept.iter().map(|&i| levels[i]).collect();
    let order = reorder_visual(&kept_levels)
        .into_iter()
        .map(|i| kept[i])
        .collect();
    (
        para.paragraph_level,
        levels.iter().map(|l| l.number()).collect(),
        order,
    )
}

fn parse_levels(field: &str) -> Vec<Option<u8>> {
    field
        .split_whitespace()
        .map(|l| if l == "x" { None } else { l.parse().ok() })
        .collect()
}

fn levels_match(expected: &[Option<u8>], actual: &[u8]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(e, a)| e.is_none_or(|e| e == *a))
}

#[test]
fn bidi_test_txt() {
    let mut levels: Vec<Option<u8>> = Vec::new();
    let mut reorder: Vec<usize> = Vec::new();
    let mut cases = 0;
    let mut failures = Vec::new();
    for (number, line) in BIDI_TEST.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Levels:") {
            levels = parse_levels(rest);
            continue;
        }
        if let Some(rest) = line.strip_prefix("@Reorder:") {
            reorder = rest
                .split_whitespace()
                .map(|i| i.parse().expect("reorder index"))
                .collect();
            continue;
        }
        if line.starts_with('@') {
            continue;
        }
        let (classes, bitset) = line.split_once(';').expect("data line has a bitset");
        let text: String = classes.split_whitespace().map(sample).collect();
        let bitset: u8 = bitset.trim().parse().expect("bitset");
        let keep: Vec<bool> = levels.iter().map(Option::is_some).collect();
        for (bit, base) in [(1, None), (2, Some(Level::ltr())), (4, Some(Level::rtl()))] {
            if bitset & bit == 0 {
                continue;
            }
            cases += 1;
            let (_, actual, order) = run_case(&text, base, &keep);
            if !levels_match(&levels, &actual) || order != reorder {
                failures.push(format!(
                    "line {}: {classes} base {base:?}: levels {actual:?} order {order:?}",
                    number + 1
                ));
            }
        }
    }
    assert!(cases > 700_000, "only {cases} cases ran");
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}

#[test]
fn bidi_character_test_txt() {
    let mut cases = 0;
    let mut failures = Vec::new();
    for (number, line) in BIDI_CHARACTER_TEST.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(';').collect();
        assert_eq!(fields.len(), 5, "line {}", number + 1);
        let text: String = fields[0]
            .split_whitespace()
            .map(|cp| {
                char::from_u32(u32::from_str_radix(cp, 16).expect("hex code point"))
                    .expect("scalar value")
            })
            .collect();
        let base = match fields[1] {
            "0" => Some(Level::ltr()),
            "1" => Some(Level::rtl()),
            _ => None,
        };
        let para_level: u8 = fields[2].parse().expect("paragraph level");
        let levels = parse_levels(fields[3]);
        let reorder: Vec<usize> = fields[4]
            .split_whitespace()
            .map(|i| i.parse().expect("reorder index"))
            .collect();
        let keep: Vec<bool> = levels.iter().map(Option::is_some).collect();
        cases += 1;
        let (actual_para, actual, order) = run_case(&text, base, &keep);
        if actual_para.number() != para_level || !levels_match(&levels, &actual) || order != reorder
        {
            failures.push(format!(
                "line {}: para {} levels {actual:?} order {order:?}",
                number + 1,
                actual_para.number()
            ));
        }
    }
    assert!(cases > 90_000, "only {cases} cases ran");
    assert!(
        failures.is_empty(),
        "{} of {cases} cases failed, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}

#[test]
fn original_classes_come_from_icu_data() {
    // u+fdd0 is a noncharacter: bn in DerivedBidiClass.txt, which
    // unicode-bidi's own tables get wrong (servo/unicode-bidi#143).
    let para = resolve_levels("\u{fdd0}", Some(Level::ltr()));
    assert_eq!(para.original_classes[0], BidiClass::BN);
}
