//! font matching, system fallback, font ids and metrics.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

use fontique::{
    Attributes, Blob, CharmapIndex, Collection, CollectionOptions, FallbackKey, FamilyId,
    FontWeight, FontWidth, Language, SourceCache,
};
use read_fonts::model::Font as ModelFont;
use read_fonts::types::Tag;
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};

use crate::Script;

/// identifies one font instance (file, collection index, variation location
/// and synthesis) for the lifetime of a [`FontContext`]. display list glyph
/// runs carry it as a plain u32.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(u32);

impl FontId {
    pub const fn from_u32(id: u32) -> Self {
        Self(id)
    }

    pub const fn to_u32(self) -> u32 {
        self.0
    }
}

/// https://drafts.csswg.org/css-fonts-4/#generic-font-families
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GenericFamily {
    Serif,
    SansSerif,
    Monospace,
    Cursive,
    Fantasy,
    SystemUi,
    UiSerif,
    UiSansSerif,
    UiMonospace,
    UiRounded,
    Emoji,
    Math,
    Fangsong,
}

impl GenericFamily {
    fn to_fontique(self) -> fontique::GenericFamily {
        use fontique::GenericFamily as F;
        match self {
            Self::Serif => F::Serif,
            Self::SansSerif => F::SansSerif,
            Self::Monospace => F::Monospace,
            Self::Cursive => F::Cursive,
            Self::Fantasy => F::Fantasy,
            Self::SystemUi => F::SystemUi,
            Self::UiSerif => F::UiSerif,
            Self::UiSansSerif => F::UiSansSerif,
            Self::UiMonospace => F::UiMonospace,
            Self::UiRounded => F::UiRounded,
            Self::Emoji => F::Emoji,
            Self::Math => F::Math,
            Self::Fangsong => F::FangSong,
        }
    }
}

/// one entry of a computed `font-family` list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily<'a> {
    Named(&'a str),
    Generic(GenericFamily),
}

/// computed `font-style`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FontStyle {
    #[default]
    Normal,
    Italic,
    /// oblique angle in degrees.
    Oblique(f32),
}

/// the font properties that select a face, in computed-value form.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontQuery<'a> {
    pub families: &'a [FontFamily<'a>],
    /// computed `font-weight`, 1 to 1000.
    pub weight: f32,
    pub style: FontStyle,
    /// computed `font-width` (`font-stretch`) as a percentage, 100 is normal.
    pub stretch: f32,
    /// bcp 47 content language; picks between han fallback fonts and
    /// language-specific shaping.
    pub language: Option<&'a str>,
}

impl Default for FontQuery<'_> {
    fn default() -> Self {
        Self {
            families: &[],
            weight: 400.0,
            style: FontStyle::Normal,
            stretch: 100.0,
            language: None,
        }
    }
}

impl FontQuery<'_> {
    fn attributes(&self) -> Attributes {
        let style = match self.style {
            FontStyle::Normal => fontique::FontStyle::Normal,
            FontStyle::Italic => fontique::FontStyle::Italic,
            FontStyle::Oblique(angle) => fontique::FontStyle::Oblique(Some(angle)),
        };
        Attributes::new(
            FontWidth::from_percentage(self.stretch),
            style,
            FontWeight::new(self.weight),
        )
    }
}

/// font-wide metrics scaled to a font size, in css px.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontMetrics {
    pub units_per_em: u16,
    /// distance from the baseline up to the ascent line, positive.
    pub ascent: f32,
    /// distance from the baseline down to the descent line, positive.
    pub descent: f32,
    pub line_gap: f32,
    pub x_height: Option<f32>,
    pub cap_height: Option<f32>,
    /// position of the top of the underline relative to the baseline, y up as
    /// the font stores it (negative is below the baseline).
    pub underline_offset: Option<f32>,
    pub underline_thickness: Option<f32>,
    /// position of the strikeout relative to the baseline, y up.
    pub strikeout_offset: Option<f32>,
    pub strikeout_thickness: Option<f32>,
}

/// what a renderer needs to draw glyphs of a [`FontId`].
#[derive(Clone, Debug)]
pub struct FontData {
    /// the whole font file. shared, cheap to clone, and `Blob::id` is stable
    /// for the file so the compositor can cache by it.
    pub blob: Blob<u8>,
    /// index of the face in a collection file (ttc/otc).
    pub index: u32,
    /// variation axis settings in user space, as (tag, value).
    pub variations: Vec<([u8; 4], f32)>,
    /// the face is lighter than requested; draw with synthetic bold.
    pub embolden: bool,
    /// the face is upright but oblique was requested; skew by this angle in
    /// degrees.
    pub skew: Option<f32>,
}

pub(crate) struct LoadedFont {
    pub(crate) data: FontData,
    pub(crate) font: ModelFont,
    charmap: CharmapIndex,
    /// metrics at one unit per em, scaled on request.
    unscaled: skrifa::metrics::Metrics,
}

impl LoadedFont {
    pub(crate) fn maps(&self, ch: char) -> bool {
        self.charmap
            .charmap(self.data.blob.as_ref())
            .and_then(|map| map.map(ch))
            .is_some_and(|glyph| glyph != 0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AttributesKey {
    weight: u32,
    stretch: u32,
    style: u32,
}

impl AttributesKey {
    pub(crate) fn new(query: &FontQuery<'_>) -> Self {
        let style = match query.style {
            FontStyle::Normal => 0,
            FontStyle::Italic => 1,
            // oblique angles are finite css angles; keep their bits apart
            // from the two keyword values.
            FontStyle::Oblique(angle) => angle.to_bits() | 2,
        };
        Self {
            weight: query.weight.to_bits(),
            stretch: query.stretch.to_bits(),
            style,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct InstanceKey {
    blob: u64,
    index: u32,
    variations: Vec<([u8; 4], u32)>,
    embolden: bool,
    skew: Option<u32>,
}

/// owns the system font collection and every font the renderer has used.
///
/// one per renderer thread. fonts are never unloaded, so a [`FontId`] stays
/// valid for the life of the context.
pub struct FontContext {
    collection: Collection,
    source_cache: SourceCache,
    pub(crate) fonts: Vec<LoadedFont>,
    instances: HashMap<InstanceKey, FontId>,
    family_fonts: HashMap<(FamilyId, AttributesKey), Option<FontId>>,
    pub(crate) plans: crate::shape::PlanCache,
}

impl Default for FontContext {
    fn default() -> Self {
        Self::new()
    }
}

impl FontContext {
    /// a context over the operating system's installed fonts.
    pub fn new() -> Self {
        Self {
            collection: Collection::new(CollectionOptions {
                shared: false,
                system_fonts: true,
            }),
            source_cache: SourceCache::default(),
            fonts: Vec::new(),
            instances: HashMap::new(),
            family_fonts: HashMap::new(),
            plans: crate::shape::PlanCache::default(),
        }
    }

    /// the bytes and instance settings behind `font`.
    pub fn font_data(&self, font: FontId) -> Option<&FontData> {
        self.fonts.get(font.0 as usize).map(|f| &f.data)
    }

    /// true when `font` has a glyph for `ch` in its character map.
    pub fn has_glyph(&self, font: FontId, ch: char) -> bool {
        self.fonts.get(font.0 as usize).is_some_and(|f| f.maps(ch))
    }

    /// font-wide metrics of `font` at `size` px.
    pub fn metrics(&self, font: FontId, size: f32) -> Option<FontMetrics> {
        let m = &self.fonts.get(font.0 as usize)?.unscaled;
        let scale = if m.units_per_em == 0 {
            0.0
        } else {
            size / f32::from(m.units_per_em)
        };
        Some(FontMetrics {
            units_per_em: m.units_per_em,
            ascent: m.ascent * scale,
            descent: -m.descent * scale,
            line_gap: m.leading * scale,
            x_height: m.x_height.map(|v| v * scale),
            cap_height: m.cap_height.map(|v| v * scale),
            underline_offset: m.underline.map(|d| d.offset * scale),
            underline_thickness: m.underline.map(|d| d.thickness * scale),
            strikeout_offset: m.strikeout.map(|d| d.offset * scale),
            strikeout_thickness: m.strikeout.map(|d| d.thickness * scale),
        })
    }

    /// the first available font for `query`
    /// (https://drafts.csswg.org/css-fonts-4/#first-available-font): the
    /// first face in the family list that loads, else the ua default
    /// sans-serif face, else any fallback face.
    pub fn first_available_font(&mut self, query: &FontQuery<'_>) -> Option<FontId> {
        let families = self.query_families(query);
        let attrs = AttributesKey::new(query);
        for family in families {
            if let Some(font) = self.family_font(family, attrs, query) {
                return Some(font);
            }
        }
        let fallback = self.fallback_families(Script::LATIN, query.language);
        fallback
            .into_iter()
            .find_map(|family| self.family_font(family, attrs, query))
    }

    /// the family ids of the author list followed by the ua default
    /// (sans-serif), in order.
    pub(crate) fn query_families(&mut self, query: &FontQuery<'_>) -> Vec<FamilyId> {
        let mut out = Vec::new();
        for family in query.families {
            match family {
                FontFamily::Named(name) => {
                    if let Some(id) = self.collection.family_id(name) {
                        out.push(id);
                    }
                }
                FontFamily::Generic(generic) => {
                    out.extend(self.collection.generic_families(generic.to_fontique()));
                }
            }
        }
        out.extend(
            self.collection
                .generic_families(fontique::GenericFamily::SansSerif),
        );
        out
    }

    pub(crate) fn generic_families(&mut self, generic: GenericFamily) -> Vec<FamilyId> {
        self.collection
            .generic_families(generic.to_fontique())
            .collect()
    }

    /// system fallback families for a script
    /// (https://drafts.csswg.org/css-fonts-4/#system-fallback).
    pub(crate) fn fallback_families(
        &mut self,
        script: Script,
        language: Option<&str>,
    ) -> Vec<FamilyId> {
        let language = language.and_then(|lang| Language::parse(lang).ok());
        let key = FallbackKey::new(
            fontique::Script::from_bytes(script.to_bytes()),
            language.as_ref(),
        );
        self.collection.fallback_families(key).collect()
    }

    /// the face of `family` that css font matching picks for `query`
    /// (https://drafts.csswg.org/css-fonts-4/#font-style-matching), loaded and
    /// given an id.
    pub(crate) fn family_font(
        &mut self,
        family: FamilyId,
        attrs: AttributesKey,
        query: &FontQuery<'_>,
    ) -> Option<FontId> {
        if let Some(cached) = self.family_fonts.get(&(family, attrs)) {
            return *cached;
        }
        let font = self.load_family_font(family, query);
        self.family_fonts.insert((family, attrs), font);
        font
    }

    fn load_family_font(&mut self, family: FamilyId, query: &FontQuery<'_>) -> Option<FontId> {
        let info = self.collection.family(family)?;
        let attributes = query.attributes();
        let face = info.match_font(attributes.width, attributes.style, attributes.weight, true)?;
        let blob = face.load(Some(&mut self.source_cache))?;
        let synthesis = face.synthesis(attributes.width, attributes.style, attributes.weight);
        let variations: Vec<([u8; 4], f32)> = synthesis
            .variation_settings()
            .iter()
            .map(|(tag, value)| (tag.to_be_bytes(), *value))
            .collect();
        let key = InstanceKey {
            blob: blob.id(),
            index: face.index(),
            variations: variations
                .iter()
                .map(|(tag, value)| (*tag, value.to_bits()))
                .collect(),
            embolden: synthesis.embolden(),
            skew: synthesis.skew().map(f32::to_bits),
        };
        match self.instances.entry(key) {
            Entry::Occupied(entry) => Some(*entry.get()),
            Entry::Vacant(entry) => {
                let shared: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(blob.clone());
                let default = ModelFont::new(read_fonts::model::Blob::from(shared), face.index())?;
                let font = if variations.is_empty() {
                    default
                } else {
                    default
                        .instance_builder()
                        .variations(
                            variations
                                .iter()
                                .map(|(tag, value)| (Tag::new(tag), *value)),
                        )
                        .build()
                };
                let font_ref = skrifa::FontRef::from_index(blob.as_ref(), face.index()).ok()?;
                let unscaled =
                    font_ref.metrics(Size::unscaled(), LocationRef::new(font.normalized_coords()));
                let id = FontId(u32::try_from(self.fonts.len()).ok()?);
                self.fonts.push(LoadedFont {
                    data: FontData {
                        blob,
                        index: face.index(),
                        variations,
                        embolden: synthesis.embolden(),
                        skew: synthesis.skew(),
                    },
                    font,
                    charmap: face.charmap_index(),
                    unscaled,
                });
                entry.insert(id);
                Some(id)
            }
        }
    }

    /// true when some installed face maps `ch`, whether or not fallback
    /// would reach it. loads every system font, so tests only.
    #[cfg(test)]
    pub(crate) fn any_system_font_maps(&mut self, ch: char) -> bool {
        let families: Vec<FamilyId> = self.collection.family_ids().collect();
        families.into_iter().any(|family| {
            let Some(info) = self.collection.family(family) else {
                return false;
            };
            info.fonts().iter().any(|face| {
                face.load(Some(&mut self.source_cache)).is_some_and(|blob| {
                    face.charmap_index()
                        .charmap(blob.as_ref())
                        .and_then(|map| map.map(ch))
                        .is_some_and(|glyph| glyph != 0)
                })
            })
        })
    }
}
