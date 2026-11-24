//! chrome text: one system ui font found through fontique, shaped with
//! harfrust and drawn by the compositor as glyph runs.

use std::sync::Arc;

use fontique::{Collection, CollectionOptions, GenericFamily, QueryFamily, SourceCache};
use sp_paint::{Color, FontId, Glyph, GlyphRun, Point};

pub(crate) struct ChromeFont {
    pub blob: fontique::Blob<u8>,
    pub index: u32,
    font: harfrust::Font,
    units_per_em: f32,
    ascent: f32,
    descent: f32,
}

/// a shaped single line of text, positioned with its baseline at y = 0 and
/// its start at x = 0.
pub(crate) struct ShapedLine {
    pub glyphs: Vec<Glyph>,
    pub width: f32,
    /// x of the caret before each char boundary, in increasing byte order,
    /// ending with the boundary at the end of the text.
    pub carets: Vec<(usize, f32)>,
}

impl ShapedLine {
    pub fn caret_x(&self, byte: usize) -> f32 {
        match self.carets.binary_search_by_key(&byte, |&(b, _)| b) {
            Ok(i) => self.carets[i].1,
            Err(i) => self.carets.get(i).map_or(self.width, |c| c.1),
        }
    }

    /// the char boundary closest to `x`.
    pub fn hit(&self, x: f32) -> usize {
        self.carets
            .iter()
            .min_by(|a, b| (a.1 - x).abs().total_cmp(&(b.1 - x).abs()))
            .map_or(0, |c| c.0)
    }

    pub fn run(&self, font: FontId, size: f32, color: Color, origin: Point) -> GlyphRun {
        GlyphRun {
            font,
            size,
            color,
            glyphs: self
                .glyphs
                .iter()
                .map(|g| Glyph {
                    id: g.id,
                    position: Point::new(origin.x + g.position.x, origin.y + g.position.y),
                })
                .collect(),
        }
    }
}

impl ChromeFont {
    /// the platform's ui font, or its sans-serif one.
    pub fn load_system() -> Option<Self> {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        let mut cache = SourceCache::default();
        let mut query = collection.query(&mut cache);
        query.set_families([
            QueryFamily::Generic(GenericFamily::SystemUi),
            QueryFamily::Generic(GenericFamily::SansSerif),
        ]);
        let found = query.first_available_font()?;
        Self::from_blob(found.blob, found.index)
    }

    pub fn from_blob(blob: fontique::Blob<u8>, index: u32) -> Option<Self> {
        let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(blob.clone());
        let font = harfrust::Font::new(bytes, index)?;
        let metrics = font.metrics();
        let units_per_em = f32::from(metrics.units_per_em.max(1));
        let line = metrics
            .typo_line
            .filter(|_| metrics.use_typo_metrics)
            .or(metrics.hhea_line);
        let (ascent, descent) = line.map_or((units_per_em * 0.8, units_per_em * 0.2), |l| {
            (l.ascender.to_f32(), -l.descender.to_f32())
        });
        Some(Self {
            blob,
            index,
            font,
            units_per_em,
            ascent,
            descent,
        })
    }

    pub fn ascent(&self, size: f32) -> f32 {
        self.ascent * size / self.units_per_em
    }

    pub fn descent(&self, size: f32) -> f32 {
        self.descent * size / self.units_per_em
    }

    pub fn shape(&self, text: &str, size: f32) -> ShapedLine {
        let scale = size / self.units_per_em;
        let shaper = harfrust::ShaperFont::new(&self.font);
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(text);
        buffer.guess_segment_properties();
        let mut glyphs = Vec::new();
        // (cluster byte, pen x before the glyph), in visual order.
        let mut starts: Vec<(usize, f32)> = Vec::new();
        let mut x = 0.0;
        if harfrust::shape(&shaper, &mut buffer, harfrust::ShapeOptions::new()).is_ok() {
            for (info, pos) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
                glyphs.push(Glyph {
                    id: info.glyph_id,
                    position: Point::new(
                        x + pos.x_offset as f32 * scale,
                        -(pos.y_offset as f32) * scale,
                    ),
                });
                starts.push((info.cluster as usize, x));
                x += pos.x_advance as f32 * scale;
            }
        }
        let width = x;
        // the url bar is laid out left to right, so cluster order is visual
        // order and the caret before a boundary sits at the first glyph whose
        // cluster starts at or after it.
        let mut carets = Vec::with_capacity(text.len() + 1);
        let mut g = 0;
        for (b, _) in text.char_indices().chain([(text.len(), ' ')]) {
            while g < starts.len() && starts[g].0 < b {
                g += 1;
            }
            carets.push((b, starts.get(g).map_or(width, |s| s.1)));
        }
        ShapedLine {
            glyphs,
            width,
            carets,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Option<ChromeFont> {
        let font = ChromeFont::load_system();
        if font.is_none() {
            eprintln!("skipping: no system ui or sans-serif font found");
        }
        font
    }

    #[test]
    fn shaping_produces_increasing_carets() {
        let Some(font) = font() else { return };
        let line = font.shape("super://newtab", 14.0);
        assert!(!line.glyphs.is_empty());
        assert!(line.width > 0.0);
        assert_eq!(line.carets.len(), "super://newtab".len() + 1);
        assert!(line.carets.windows(2).all(|w| w[0].1 <= w[1].1));
        assert_eq!(line.caret_x(0), 0.0);
        assert_eq!(line.caret_x(14), line.width);
    }

    #[test]
    fn hit_finds_the_nearest_boundary() {
        let Some(font) = font() else { return };
        let line = font.shape("abc", 20.0);
        assert_eq!(line.hit(-5.0), 0);
        assert_eq!(line.hit(line.width + 5.0), 3);
        let mid_b = (line.caret_x(1) + line.caret_x(2)) / 2.0 - 0.1;
        assert_eq!(line.hit(mid_b), 1);
    }

    #[test]
    fn empty_text_has_one_caret() {
        let Some(font) = font() else { return };
        let line = font.shape("", 14.0);
        assert_eq!(line.carets, vec![(0, 0.0)]);
    }
}
