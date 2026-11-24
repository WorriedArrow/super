//! uax #9 levels through unicode-bidi with bidi classes from icu_properties.
//!
//! per the decision on issue #2, only `ParagraphBidiInfo::new_with_data_source`
//! and `BidiInfo::reorder_visual` are used from unicode-bidi. the per-line
//! methods (`visual_runs`, `reorder_line`, `reordered_levels`, `has_rtl`) clone
//! the paragraph per call and `visual_runs` can panic on page input in 0.3.18.

use std::ops::Range;

use icu_properties::CodePointMapData;
use icu_properties::props::BidiClass as IcuBidiClass;
use unicode_bidi::{BidiInfo, ParagraphBidiInfo};

pub use unicode_bidi::{BidiClass, Level};

/// resolved levels for one bidi paragraph.
///
/// `levels` and `original_classes` have one entry per utf-8 byte of the text,
/// so a byte offset indexes both directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BidiParagraph {
    pub levels: Vec<Level>,
    pub original_classes: Vec<BidiClass>,
    pub paragraph_level: Level,
}

/// resolves embedding levels for `text` as a single paragraph (rules p2-p3,
/// x1-x10, w1-w7, n0-n2, i1-i2).
///
/// `base` is the paragraph embedding level from css `direction`; `None` runs
/// p2-p3 on the text, which is what `unicode-bidi: plaintext` asks for
/// (https://drafts.csswg.org/css-writing-modes-3/#bidi-para-direction).
pub fn resolve_levels(text: &str, base: Option<Level>) -> BidiParagraph {
    let data = CodePointMapData::<IcuBidiClass>::new();
    let info = ParagraphBidiInfo::new_with_data_source(&data, text, base);
    BidiParagraph {
        levels: info.levels,
        original_classes: info.original_classes,
        paragraph_level: info.paragraph_level,
    }
}

/// returns the levels of `line` (a byte range of `text`) after rule l1.
///
/// https://www.unicode.org/reports/tr9/#L1, together with the treatment of
/// characters removed by x9 from
/// https://www.unicode.org/reports/tr9/#Retaining_Explicit_Formatting_Characters:
/// they take part in the trailing whitespace sequences of l1.
pub fn line_levels(text: &str, paragraph: &BidiParagraph, line: Range<usize>) -> Vec<Level> {
    let mut levels = paragraph.levels[line.clone()].to_vec();
    let para = paragraph.paragraph_level;
    let Some(line_text) = text.get(line.clone()) else {
        return levels;
    };
    // walking backwards, `trailing` is true while every character after the
    // current one up to the line end or the next s/b is whitespace-like.
    let mut trailing = true;
    for (offset, ch) in line_text.char_indices().rev() {
        let class = paragraph.original_classes[line.start + offset];
        let reset = match class {
            // l1 items 1 and 2: segment and paragraph separators.
            BidiClass::S | BidiClass::B => {
                trailing = true;
                true
            }
            // l1 items 3 and 4: whitespace and isolate formatting characters
            // before a separator or at the end of the line, plus x9-removed
            // characters inside such sequences.
            BidiClass::WS
            | BidiClass::FSI
            | BidiClass::LRI
            | BidiClass::RLI
            | BidiClass::PDI
            | BidiClass::BN
            | BidiClass::LRE
            | BidiClass::RLE
            | BidiClass::LRO
            | BidiClass::RLO
            | BidiClass::PDF => trailing,
            _ => {
                trailing = false;
                false
            }
        };
        if reset {
            for level in &mut levels[offset..offset + ch.len_utf8()] {
                *level = para;
            }
        }
    }
    levels
}

/// rule l2: maps visual position to logical index for a sequence of levels.
///
/// the levels can belong to anything sp_layout reorders (characters or line
/// items), so this is the static form that never touches paragraph state.
pub fn reorder_visual(levels: &[Level]) -> Vec<usize> {
    BidiInfo::reorder_visual(levels)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn char_levels(text: &str, levels: &[Level]) -> Vec<u8> {
        text.char_indices()
            .map(|(i, _)| levels[i].number())
            .collect()
    }

    #[test]
    fn mixed_ltr_paragraph() {
        // "abc " then hebrew alef bet gimel, then " 123". the digits follow
        // an r, so w7 leaves them en at level 2 (i2) and n1 puts the space
        // between r and en at level 1.
        let text = "abc \u{5d0}\u{5d1}\u{5d2} 123";
        let para = resolve_levels(text, Some(Level::ltr()));
        assert_eq!(para.paragraph_level, Level::ltr());
        assert_eq!(
            char_levels(text, &para.levels),
            [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2]
        );
    }

    #[test]
    fn mixed_rtl_paragraph_with_numbers() {
        // arabic letters, a space, european digits: digits are level 2.
        let text = "\u{627}\u{644} 12";
        let para = resolve_levels(text, Some(Level::rtl()));
        assert_eq!(char_levels(text, &para.levels), [1, 1, 1, 2, 2]);
    }

    #[test]
    fn auto_direction_from_first_strong() {
        let para = resolve_levels("\u{5d0} abc", None);
        assert_eq!(para.paragraph_level, Level::rtl());
        let para = resolve_levels("abc \u{5d0}", None);
        assert_eq!(para.paragraph_level, Level::ltr());
        let para = resolve_levels("123", None);
        assert_eq!(para.paragraph_level, Level::ltr());
    }

    #[test]
    fn isolate_keeps_outside_ltr() {
        // a, rli, alef, pdi, b
        let text = "a\u{2067}\u{5d0}\u{2069}b";
        let para = resolve_levels(text, Some(Level::ltr()));
        assert_eq!(char_levels(text, &para.levels), [0, 0, 1, 0, 0]);
    }

    #[test]
    fn l1_resets_trailing_whitespace() {
        // inside an rle embedding the trailing space resolves to level 1;
        // l1 puts it back at the paragraph level.
        let text = "\u{202b}\u{5d0} ";
        let para = resolve_levels(text, Some(Level::ltr()));
        assert_eq!(char_levels(text, &para.levels)[1..], [1, 1]);
        let line = line_levels(text, &para, 0..text.len());
        assert_eq!(char_levels(text, &line), [0, 1, 0]);
    }

    #[test]
    fn l1_resets_whitespace_before_tab() {
        let text = "\u{5d0} \t\u{5d1}";
        let para = resolve_levels(text, Some(Level::ltr()));
        let line = line_levels(text, &para, 0..text.len());
        assert_eq!(char_levels(text, &line), [1, 0, 0, 1]);
    }

    #[test]
    fn l2_reverses_rtl_run() {
        let text = "ab\u{5d0}\u{5d1}c";
        let para = resolve_levels(text, Some(Level::ltr()));
        let levels: Vec<Level> = text.char_indices().map(|(i, _)| para.levels[i]).collect();
        assert_eq!(reorder_visual(&levels), [0, 1, 3, 2, 4]);
    }

    #[test]
    fn deep_nesting_does_not_panic() {
        let mut text = String::new();
        for _ in 0..200 {
            text.push('\u{2067}');
        }
        text.push('1');
        let para = resolve_levels(&text, Some(Level::ltr()));
        let line = line_levels(&text, &para, 0..text.len());
        let levels: Vec<Level> = text.char_indices().map(|(i, _)| line[i]).collect();
        assert_eq!(reorder_visual(&levels).len(), levels.len());
    }
}
