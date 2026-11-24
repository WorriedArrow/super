//! line break opportunities (uax #14 as tailored by css-text-3) and grapheme
//! and word boundaries (uax #29), all from icu_segmenter.

use icu_locale_core::LanguageIdentifier;
use icu_properties::CodePointMapData;
use icu_properties::props::LineBreak as LineBreakClass;
use icu_segmenter::options::{
    LineBreakOptions as IcuLineBreakOptions, LineBreakStrictness, LineBreakWordOption,
    WordBreakInvariantOptions,
};
use icu_segmenter::{GraphemeClusterSegmenter, LineSegmenter, WordSegmenter};

/// css `line-break` (https://drafts.csswg.org/css-text-3/#line-break-property).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LineBreak {
    #[default]
    Auto,
    Loose,
    Normal,
    Strict,
    Anywhere,
}

/// css `word-break` (https://drafts.csswg.org/css-text-3/#word-break-property).
///
/// the deprecated `break-word` value is `normal` here; sp_layout applies its
/// `overflow-wrap: anywhere` half.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WordBreak {
    #[default]
    Normal,
    BreakAll,
    KeepAll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct LineBreakOptions<'a> {
    pub line_break: LineBreak,
    pub word_break: WordBreak,
    /// bcp 47 content language. `line-break: normal` and `loose` allow more
    /// breaks for japanese and chinese.
    pub language: Option<&'a str>,
}

/// a point between two characters where a line may (or must) end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BreakOpportunity {
    /// byte offset of the first character after the break.
    pub offset: usize,
    /// a forced break: after a uax #14 bk, cr, lf or nl character, or at the
    /// end of the text (lb3).
    pub mandatory: bool,
}

/// appends the line break opportunities of `text` to `out`, in order.
///
/// https://drafts.csswg.org/css-text-3/#line-breaking. the start of the text
/// is never an opportunity and the end always is.
pub fn line_break_opportunities(
    text: &str,
    options: &LineBreakOptions<'_>,
    out: &mut Vec<BreakOpportunity>,
) {
    let langid = options
        .language
        .and_then(|lang| LanguageIdentifier::try_from_str(lang).ok());
    let mut icu_options = IcuLineBreakOptions::default();
    // css-text-3 leaves `auto` to the ua; it gets the `normal` rule set.
    icu_options.strictness = Some(match options.line_break {
        LineBreak::Auto | LineBreak::Normal => LineBreakStrictness::Normal,
        LineBreak::Loose => LineBreakStrictness::Loose,
        LineBreak::Strict => LineBreakStrictness::Strict,
        LineBreak::Anywhere => LineBreakStrictness::Anywhere,
    });
    icu_options.word_option = Some(match options.word_break {
        WordBreak::Normal => LineBreakWordOption::Normal,
        WordBreak::BreakAll => LineBreakWordOption::BreakAll,
        WordBreak::KeepAll => LineBreakWordOption::KeepAll,
    });
    icu_options.content_locale = langid.as_ref();
    let classes = CodePointMapData::<LineBreakClass>::new();
    for offset in LineSegmenter::new_auto(icu_options).segment_str(text) {
        if offset == 0 {
            continue;
        }
        let mandatory = offset == text.len()
            || text[..offset].chars().next_back().is_some_and(|ch| {
                matches!(
                    classes.get(ch),
                    LineBreakClass::MandatoryBreak
                        | LineBreakClass::CarriageReturn
                        | LineBreakClass::LineFeed
                        | LineBreakClass::NextLine
                )
            });
        out.push(BreakOpportunity { offset, mandatory });
    }
}

/// extended grapheme cluster boundaries of `text` as byte offsets, including
/// 0 and `text.len()` (https://www.unicode.org/reports/tr29/#Grapheme_Cluster_Boundaries).
pub fn grapheme_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    GraphemeClusterSegmenter::new().segment_str(text)
}

/// word boundaries of `text` as byte offsets, including 0 and `text.len()`
/// (https://www.unicode.org/reports/tr29/#Word_Boundaries), with dictionary
/// or lstm segmentation for scripts written without spaces.
pub fn word_boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    WordSegmenter::new_auto(WordBreakInvariantOptions::default()).segment_str(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn breaks(text: &str, options: LineBreakOptions<'_>) -> Vec<(usize, bool)> {
        let mut out = Vec::new();
        line_break_opportunities(text, &options, &mut out);
        out.iter().map(|b| (b.offset, b.mandatory)).collect()
    }

    #[test]
    fn breaks_after_spaces() {
        assert_eq!(
            breaks("hello big world", LineBreakOptions::default()),
            [(6, false), (10, false), (15, true)]
        );
    }

    #[test]
    fn hard_breaks_are_mandatory() {
        assert_eq!(
            breaks("a\nb\r\nc", LineBreakOptions::default()),
            [(2, true), (5, true), (6, true)]
        );
    }

    #[test]
    fn no_break_before_closing_punctuation() {
        assert_eq!(
            breaks("(a) b.", LineBreakOptions::default()),
            [(4, false), (6, true)]
        );
    }

    #[test]
    fn empty_text_has_no_opportunities() {
        assert_eq!(breaks("", LineBreakOptions::default()), []);
    }

    #[test]
    fn cjk_breaks_between_ideographs() {
        // three ideographs, three bytes each.
        assert_eq!(
            breaks("漢字書", LineBreakOptions::default()),
            [(3, false), (6, false), (9, true)]
        );
    }

    #[test]
    fn keep_all_suppresses_cjk_breaks() {
        let options = LineBreakOptions {
            word_break: WordBreak::KeepAll,
            ..Default::default()
        };
        assert_eq!(breaks("漢字書", options), [(9, true)]);
    }

    #[test]
    fn break_all_breaks_inside_words() {
        let options = LineBreakOptions {
            word_break: WordBreak::BreakAll,
            ..Default::default()
        };
        assert_eq!(breaks("abc", options), [(1, false), (2, false), (3, true)]);
    }

    #[test]
    fn anywhere_breaks_everywhere() {
        let options = LineBreakOptions {
            line_break: LineBreak::Anywhere,
            ..Default::default()
        };
        assert_eq!(breaks("a-b", options), [(1, false), (2, false), (3, true)]);
    }

    #[test]
    fn strict_and_normal_differ_on_small_kana() {
        // small kana (class cj) before it: strict forbids the break, normal
        // allows it (https://drafts.csswg.org/css-text-3/#line-break-property).
        let text = "あぁ";
        let strict = LineBreakOptions {
            line_break: LineBreak::Strict,
            language: Some("ja"),
            ..Default::default()
        };
        let normal = LineBreakOptions {
            line_break: LineBreak::Normal,
            language: Some("ja"),
            ..Default::default()
        };
        assert_eq!(breaks(text, strict), [(6, true)]);
        assert_eq!(breaks(text, normal), [(3, false), (6, true)]);
    }

    #[test]
    fn grapheme_clusters() {
        // e + combining acute, a flag (two regional indicators), a family
        // emoji joined with zwj, and a hangul syllable from jamo.
        let text = "e\u{301}\u{1f1eb}\u{1f1f7}\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{1100}\u{1161}x";
        let bounds: Vec<usize> = grapheme_boundaries(text).collect();
        let e = "e\u{301}".len();
        let flag = e + "\u{1f1eb}\u{1f1f7}".len();
        let family = flag + "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}".len();
        let jamo = family + "\u{1100}\u{1161}".len();
        assert_eq!(bounds, [0, e, flag, family, jamo, text.len()]);
    }

    #[test]
    fn grapheme_devanagari_conjunct_stays_together() {
        // ka + virama + ssa forms one extended grapheme cluster since unicode
        // 15.1 (gb9c).
        let text = "\u{915}\u{94d}\u{937}";
        let bounds: Vec<usize> = grapheme_boundaries(text).collect();
        assert_eq!(bounds, [0, text.len()]);
    }

    #[test]
    fn word_boundaries_latin() {
        let bounds: Vec<usize> = word_boundaries("can't stop").collect();
        assert_eq!(bounds, [0, 5, 6, 10]);
    }
}
