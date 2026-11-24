//! fonts and text for inline layout: font matching with system fallback,
//! itemization, shaping, line break opportunities, grapheme and word
//! segmentation, and bidi levels.
//!
//! the expected flow for one paragraph in sp_layout:
//! 1. [`bidi::resolve_levels`] on the paragraph text (with css bidi control
//!    codes inserted).
//! 2. [`FontContext::itemize`] per styled range, giving [`TextRun`]s.
//! 3. [`FontContext::shape`] per run, giving [`ShapedRun`]s to measure.
//! 4. [`segment::line_break_opportunities`] to choose where lines end.
//! 5. per line, [`bidi::line_levels`] and [`bidi::reorder_visual`].

pub mod bidi;
mod font;
mod itemize;
pub mod segment;
mod shape;
#[cfg(test)]
mod tests;

pub use fontique::Blob;

pub use font::{
    FontContext, FontData, FontFamily, FontId, FontMetrics, FontQuery, FontStyle, GenericFamily,
};
pub use itemize::TextRun;
pub use shape::{FontFeature, Glyph, ShapeOptions, ShapedRun};

/// an iso 15924 script code, e.g. `Latn`, `Arab`, `Zyyy` for common.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Script([u8; 4]);

impl Script {
    pub const COMMON: Self = Self(*b"Zyyy");
    pub const LATIN: Self = Self(*b"Latn");

    pub const fn from_bytes(code: [u8; 4]) -> Self {
        Self(code)
    }

    pub const fn to_bytes(self) -> [u8; 4] {
        self.0
    }
}
