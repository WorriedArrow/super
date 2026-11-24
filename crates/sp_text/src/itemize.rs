//! splitting text into runs that share a script, a bidi level and a font.

use std::collections::HashMap;
use std::ops::Range;

use fontique::FamilyId;
use icu_properties::props::{
    DefaultIgnorableCodePoint, EmojiPresentation, GeneralCategory, Script as IcuScript,
};
use icu_properties::{CodePointMapData, CodePointSetData, PropertyNamesShort};
use icu_segmenter::GraphemeClusterSegmenter;
use unicode_bidi::Level;

use crate::Script;
use crate::font::{AttributesKey, FontContext, FontId, FontQuery, GenericFamily};

/// a maximal piece of text that can be shaped in one call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextRun {
    /// byte range in the text passed to [`FontContext::itemize`].
    pub range: Range<usize>,
    pub font: FontId,
    pub script: Script,
    pub level: Level,
}

struct Cluster {
    range: Range<usize>,
    script: Option<Script>,
    emoji: bool,
}

/// the iso 15924 code of a character's script property, or `None` for
/// common, inherited and unknown, which take the script of their context
/// (https://www.unicode.org/reports/tr24/#Nonspacing_Marks and
/// https://www.unicode.org/reports/tr24/#Common).
fn char_script(ch: char) -> Option<Script> {
    let script = CodePointMapData::<IcuScript>::new().get(ch);
    if matches!(
        script,
        IcuScript::Common | IcuScript::Inherited | IcuScript::Unknown
    ) {
        return None;
    }
    let name = PropertyNamesShort::<IcuScript>::new().get_locale_script(script)?;
    let bytes: [u8; 4] = name.as_str().as_bytes().try_into().ok()?;
    Some(Script::from_bytes(bytes))
}

/// characters that need no glyph of their own when choosing a font for a
/// cluster: default ignorables (joiners, variation selectors, bidi controls)
/// and c0/c1 controls such as tab and newline.
fn ignorable_for_coverage(ch: char) -> bool {
    CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(ch)
        || CodePointMapData::<GeneralCategory>::new().get(ch) == GeneralCategory::Control
}

/// https://www.unicode.org/reports/tr51/#Presentation_Style: emoji
/// presentation by default, or text-default emoji followed by u+fe0f.
fn is_emoji_cluster(cluster: &str) -> bool {
    let mut chars = cluster.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if cluster.contains('\u{fe0e}') {
        return false;
    }
    CodePointSetData::new::<EmojiPresentation>().contains(first) || cluster.contains('\u{fe0f}')
}

impl FontContext {
    /// splits `text[range]` into runs by script, bidi level and font, and
    /// appends them to `out` in logical order.
    ///
    /// `levels` holds one level per byte of `text`, as
    /// [`crate::bidi::resolve_levels`] returns them. fonts are chosen per
    /// grapheme cluster by https://drafts.csswg.org/css-fonts-4/#cluster-matching:
    /// the first face in the family list that maps every character of the
    /// cluster, then system fallback for the cluster's script, then the first
    /// face that maps the base character, then the first available font.
    pub fn itemize(
        &mut self,
        text: &str,
        range: Range<usize>,
        levels: &[Level],
        query: &FontQuery<'_>,
        out: &mut Vec<TextRun>,
    ) {
        let Some(slice) = text.get(range.clone()) else {
            return;
        };
        let mut clusters = Vec::new();
        let mut start = 0;
        for end in GraphemeClusterSegmenter::new().segment_str(slice).skip(1) {
            let cluster = &slice[start..end];
            clusters.push(Cluster {
                range: range.start + start..range.start + end,
                script: cluster.chars().find_map(char_script),
                emoji: is_emoji_cluster(cluster),
            });
            start = end;
        }
        resolve_common_scripts(&mut clusters);

        let author = self.query_families(query);
        let emoji = self.generic_families(GenericFamily::Emoji);
        let default = self.generic_families(GenericFamily::SansSerif);
        let mut candidate_lists: HashMap<(Script, bool), Vec<FamilyId>> = HashMap::new();
        let first_available = self.first_available_font(query);

        let mut previous: Option<FontId> = None;
        let mut pending: Vec<usize> = Vec::new();
        let mut fonts: Vec<Option<FontId>> = Vec::with_capacity(clusters.len());
        for (index, cluster) in clusters.iter().enumerate() {
            let script = cluster.script.unwrap_or(Script::COMMON);
            let cluster_text = &text[cluster.range.clone()];
            let needed: Vec<char> = cluster_text
                .chars()
                .filter(|ch| !ignorable_for_coverage(*ch))
                .collect();
            if needed.is_empty() {
                fonts.push(previous);
                if previous.is_none() {
                    pending.push(index);
                }
                continue;
            }
            let candidates = match candidate_lists.get(&(script, cluster.emoji)) {
                Some(list) => list.clone(),
                None => {
                    // author families, then for emoji the emoji generic ahead
                    // of script fallback, then the emoji generic and the ua
                    // default as last resorts.
                    let mut list = author.clone();
                    if cluster.emoji {
                        list.extend_from_slice(&emoji);
                    }
                    list.extend(self.fallback_families(script, query.language));
                    list.extend_from_slice(&emoji);
                    list.extend_from_slice(&default);
                    candidate_lists.insert((script, cluster.emoji), list.clone());
                    list
                }
            };

            let font = self
                .pick_font(&candidates, query, |ctx, font| {
                    needed.iter().all(|ch| ctx.has_glyph(font, *ch))
                })
                .or_else(|| {
                    self.pick_font(&candidates, query, |ctx, font| {
                        ctx.has_glyph(font, needed[0])
                    })
                })
                .or(first_available);
            for index in pending.drain(..) {
                fonts[index] = font;
            }
            fonts.push(font);
            previous = font.or(previous);
        }
        for index in pending.drain(..) {
            fonts[index] = first_available;
        }

        let mut current: Option<TextRun> = None;
        for (cluster, font) in clusters.iter().zip(fonts) {
            let Some(font) = font else {
                continue;
            };
            let script = cluster.script.unwrap_or(Script::COMMON);
            let level = levels
                .get(cluster.range.start)
                .copied()
                .unwrap_or_else(Level::ltr);
            match &mut current {
                Some(run)
                    if run.font == font
                        && run.script == script
                        && run.level == level
                        && run.range.end == cluster.range.start =>
                {
                    run.range.end = cluster.range.end;
                }
                _ => {
                    if let Some(run) = current.take() {
                        out.push(run);
                    }
                    current = Some(TextRun {
                        range: cluster.range.clone(),
                        font,
                        script,
                        level,
                    });
                }
            }
        }
        if let Some(run) = current {
            out.push(run);
        }
    }

    fn pick_font(
        &mut self,
        families: &[FamilyId],
        query: &FontQuery<'_>,
        accept: impl Fn(&FontContext, FontId) -> bool,
    ) -> Option<FontId> {
        let key = AttributesKey::new(query);
        for family in families {
            if let Some(font) = self.family_font(*family, key, query)
                && accept(self, font)
            {
                return Some(font);
            }
        }
        None
    }
}

/// gives clusters with only common or inherited characters the script of the
/// preceding cluster, or of the following one at the start of the text.
fn resolve_common_scripts(clusters: &mut [Cluster]) {
    let mut previous = None;
    for cluster in clusters.iter_mut() {
        match cluster.script {
            Some(script) => previous = Some(script),
            None => cluster.script = previous,
        }
    }
    let mut next = None;
    for cluster in clusters.iter_mut().rev() {
        match cluster.script {
            Some(script) => next = Some(script),
            None => cluster.script = next,
        }
    }
}
