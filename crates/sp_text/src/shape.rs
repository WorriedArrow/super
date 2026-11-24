//! shaping a run into positioned glyphs with harfrust.

use std::collections::HashMap;
use std::str::FromStr;

use harfrust::{Buffer, BufferFlags, Direction, Feature, ShapePlan, ShaperFont, Tag};

use crate::font::{FontContext, FontId};
use crate::itemize::TextRun;

/// an opentype feature setting from `font-feature-settings` or the
/// `font-variant-*` properties, applied to the whole run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFeature {
    pub tag: [u8; 4],
    pub value: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapeOptions<'a> {
    /// font size in css px.
    pub size: f32,
    /// bcp 47 content language, for language-specific glyph forms.
    pub language: Option<&'a str>,
    pub features: &'a [FontFeature],
}

/// one positioned glyph, in css px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub id: u32,
    /// byte offset in the text of the first character of the cluster this
    /// glyph belongs to.
    pub cluster: usize,
    /// inline-axis advance.
    pub advance: f32,
    /// offset from the pen position, x to the right.
    pub offset_x: f32,
    /// offset from the pen position, y down.
    pub offset_y: f32,
    /// breaking the text at the start of this glyph's cluster changes the
    /// shaping on at least one side, so both sides must be shaped again.
    pub unsafe_to_break: bool,
}

/// the glyphs of one run, in visual order (left to right, also for rtl runs).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapedRun {
    pub font: FontId,
    pub size: f32,
    pub rtl: bool,
    pub glyphs: Vec<Glyph>,
    /// sum of the glyph advances.
    pub advance: f32,
}

impl ShapedRun {
    /// the advance of the glyphs whose clusters start inside `range`, a byte
    /// range of the text.
    pub fn advance_of(&self, range: std::ops::Range<usize>) -> f32 {
        self.glyphs
            .iter()
            .filter(|glyph| range.contains(&glyph.cluster))
            .map(|glyph| glyph.advance)
            .sum()
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct PlanKey {
    font: FontId,
    script: [u8; 4],
    rtl: bool,
    language: Option<String>,
    features: Vec<FontFeature>,
}

#[derive(Default)]
pub(crate) struct PlanCache {
    plans: HashMap<PlanKey, ShapePlan>,
    buffer: Option<Buffer>,
}

impl FontContext {
    /// shapes `text[run.range]` with the run's font, script and direction.
    ///
    /// the text around the run is passed as context, so arabic joining and
    /// similar contextual forms see across run boundaries. `run` can be a
    /// piece of an itemized run, e.g. one line's part of it.
    pub fn shape(&mut self, text: &str, run: &TextRun, options: &ShapeOptions<'_>) -> ShapedRun {
        let rtl = run.level.is_rtl();
        let mut shaped = ShapedRun {
            font: run.font,
            size: options.size,
            rtl,
            glyphs: Vec::new(),
            advance: 0.0,
        };
        let (Some(slice), Some(before), Some(after)) = (
            text.get(run.range.clone()),
            text.get(..run.range.start),
            text.get(run.range.end..),
        ) else {
            return shaped;
        };
        let Some(loaded) = self.fonts.get(run.font.to_u32() as usize) else {
            return shaped;
        };
        let direction = if rtl {
            Direction::RightToLeft
        } else {
            Direction::LeftToRight
        };
        let script = harfrust::Script::from_iso15924_tag(Tag::new(&run.script.to_bytes()));
        let language = options
            .language
            .and_then(|lang| harfrust::Language::from_str(lang).ok());
        let features: Vec<Feature> = options
            .features
            .iter()
            .map(|f| Feature::new(Tag::new(&f.tag), f.value, ..))
            .collect();

        let key = PlanKey {
            font: run.font,
            script: run.script.to_bytes(),
            rtl,
            language: options.language.map(str::to_owned),
            features: options.features.to_vec(),
        };
        let plan = self.plans.plans.entry(key).or_insert_with(|| {
            ShapePlan::new(
                &loaded.font,
                direction,
                script,
                language.as_ref(),
                &features,
            )
        });

        let mut buffer = self.plans.buffer.take().unwrap_or_default();
        buffer.clear();
        buffer.push_str(slice);
        buffer.set_direction(direction);
        buffer.set_script(script);
        buffer.set_language(language);
        buffer.set_pre_context(before);
        buffer.set_post_context(after);
        let mut flags = BufferFlags::empty();
        if before.is_empty() {
            flags |= BufferFlags::BEGINNING_OF_TEXT;
        }
        if after.is_empty() {
            flags |= BufferFlags::END_OF_TEXT;
        }
        buffer.set_flags(flags);

        let shaper = ShaperFont::new(&loaded.font);
        let result = harfrust::shape(
            &shaper,
            &mut buffer,
            harfrust::ShapeOptions::new()
                .plan(Some(plan))
                .features(&features),
        );
        if result.is_ok() {
            let upem = loaded.font.units_per_em();
            let scale = if upem == 0 {
                0.0
            } else {
                options.size / f32::from(upem)
            };
            shaped.glyphs.reserve(buffer.len());
            for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
                let advance = pos.x_advance as f32 * scale;
                shaped.advance += advance;
                shaped.glyphs.push(Glyph {
                    id: info.glyph_id,
                    cluster: run.range.start + info.cluster as usize,
                    advance,
                    offset_x: pos.x_offset as f32 * scale,
                    offset_y: -(pos.y_offset as f32) * scale,
                    unsafe_to_break: info.unsafe_to_break(),
                });
            }
        }
        self.plans.buffer = Some(buffer);
        shaped
    }
}
