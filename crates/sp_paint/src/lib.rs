//! display list types (docs/design/engine.md, "paint").
//!
//! a display list is a flat sequence of items in painting order. clips,
//! transforms, opacity and scroll frames are pushed and then closed by
//! [`DisplayItem::Pop`], which ends whichever of them was pushed last. fonts
//! and images are referenced by id; their bytes reach the compositor once,
//! separately from the lists that use them.
//!
//! everything here is plain data so a list can cross ipc. coordinates are css
//! pixels with y pointing down.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn min_x(&self) -> f32 {
        self.origin.x
    }

    pub fn min_y(&self) -> f32 {
        self.origin.y
    }

    pub fn max_x(&self) -> f32 {
        self.origin.x + self.size.width
    }

    pub fn max_y(&self) -> f32 {
        self.origin.y + self.size.height
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.min_x() && p.x < self.max_x() && p.y >= self.min_y() && p.y < self.max_y()
    }
}

/// non-premultiplied srgb color, 8 bits per channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// elliptical corner radii, in the order top-left, top-right, bottom-right,
/// bottom-left. css-backgrounds-3 allows elliptical corners, but vello's
/// rounded rects are circular, so a single value per corner is kept for now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub const fn uniform(r: f32) -> Self {
        Self {
            top_left: r,
            top_right: r,
            bottom_right: r,
            bottom_left: r,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.top_left <= 0.0
            && self.top_right <= 0.0
            && self.bottom_right <= 0.0
            && self.bottom_left <= 0.0
    }
}

/// per-side values in css order: top, right, bottom, left.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sides<T> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy> Sides<T> {
    pub const fn all(v: T) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

/// 2d affine transform `[a b c d e f]`, mapping (x, y) to
/// (a*x + c*y + e, b*x + d*y + f), the same layout as css `matrix()`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform(pub [f32; 6]);

impl Transform {
    pub const IDENTITY: Transform = Transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub const fn translate(x: f32, y: f32) -> Self {
        Transform([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub const fn scale(sx: f32, sy: f32) -> Self {
        Transform([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// clockwise rotation by `radians` (y points down).
    pub fn rotate(radians: f32) -> Self {
        let (s, c) = radians.sin_cos();
        Transform([c, s, -s, c, 0.0, 0.0])
    }

    /// `self` first, then `next`.
    pub fn then(self, next: Transform) -> Transform {
        let [a1, b1, c1, d1, e1, f1] = self.0;
        let [a2, b2, c2, d2, e2, f2] = next.0;
        Transform([
            a2 * a1 + c2 * b1,
            b2 * a1 + d2 * b1,
            a2 * c1 + c2 * d1,
            b2 * c1 + d2 * d1,
            a2 * e1 + c2 * f1 + e2,
            b2 * e1 + d2 * f1 + f2,
        ])
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// a font the compositor has been given bytes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FontId(pub u64);

/// an image the compositor has been given pixels for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ImageId(pub u64);

/// a scrollable region whose offset the compositor owns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ScrollFrameId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Glyph {
    pub id: u32,
    /// pen position of the glyph origin on the baseline.
    pub position: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathSegment {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

/// an outline made of subpaths, as in svg path data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Path {
    pub segments: Vec<PathSegment>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FillRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        // the svg defaults for stroke-width, stroke-linecap, stroke-linejoin
        // and stroke-miterlimit.
        Self {
            width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GlyphRun {
    pub font: FontId,
    /// em size in css pixels.
    pub size: f32,
    pub color: Color,
    pub glyphs: Vec<Glyph>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum DisplayItem {
    Rect {
        rect: Rect,
        color: Color,
    },
    RoundedRect {
        rect: Rect,
        radii: CornerRadii,
        color: Color,
    },
    /// a solid border drawn inside `rect`, between its edge and the edge
    /// inset by `widths`. the inner corner radii are the outer ones reduced
    /// by the adjacent widths, per css-backgrounds-3 "corner shaping".
    Border {
        rect: Rect,
        radii: CornerRadii,
        widths: Sides<f32>,
        colors: Sides<Color>,
    },
    GlyphRun(GlyphRun),
    FillPath {
        path: Path,
        rule: FillRule,
        color: Color,
    },
    StrokePath {
        path: Path,
        style: StrokeStyle,
        color: Color,
    },
    /// the whole image scaled into `rect`.
    Image {
        rect: Rect,
        image: ImageId,
    },
    /// clips everything up to the matching pop to `rect` with `radii`.
    PushClip {
        rect: Rect,
        radii: CornerRadii,
    },
    /// applies `transform` after the current one up to the matching pop.
    PushTransform(Transform),
    /// composites everything up to the matching pop as one group with this
    /// alpha, in 0..=1.
    PushOpacity(f32),
    /// placeholder for compositor-side scrolling: clips to `clip` and
    /// translates the contents by the frame's scroll offset, which the
    /// compositor owns. `content_size` is the scrollable overflow.
    PushScrollFrame {
        id: ScrollFrameId,
        clip: Rect,
        content_size: Size,
    },
    /// ends the most recent push.
    Pop,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DisplayList {
    pub items: Vec<DisplayItem>,
}

impl DisplayList {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, item: DisplayItem) {
        self.items.push(item);
    }

    pub fn rect(&mut self, rect: Rect, color: Color) {
        self.push(DisplayItem::Rect { rect, color });
    }

    pub fn rounded_rect(&mut self, rect: Rect, radii: CornerRadii, color: Color) {
        self.push(DisplayItem::RoundedRect { rect, radii, color });
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> DisplayList {
        let mut list = DisplayList::new();
        list.rect(Rect::new(0.0, 0.0, 100.0, 50.0), Color::rgb(255, 0, 0));
        list.push(DisplayItem::PushClip {
            rect: Rect::new(5.0, 5.0, 90.0, 40.0),
            radii: CornerRadii::uniform(4.0),
        });
        list.push(DisplayItem::PushTransform(Transform::translate(3.0, 4.0)));
        list.push(DisplayItem::PushOpacity(0.5));
        list.rounded_rect(
            Rect::new(1.0, 2.0, 3.0, 4.0),
            CornerRadii::uniform(1.0),
            Color::rgba(1, 2, 3, 4),
        );
        list.push(DisplayItem::Border {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radii: CornerRadii::default(),
            widths: Sides::all(1.0),
            colors: Sides {
                top: Color::BLACK,
                right: Color::WHITE,
                bottom: Color::BLACK,
                left: Color::WHITE,
            },
        });
        list.push(DisplayItem::GlyphRun(GlyphRun {
            font: FontId(7),
            size: 16.0,
            color: Color::BLACK,
            glyphs: vec![
                Glyph {
                    id: 36,
                    position: Point::new(0.0, 12.0),
                },
                Glyph {
                    id: 37,
                    position: Point::new(9.5, 12.0),
                },
            ],
        }));
        let path = Path {
            segments: vec![
                PathSegment::MoveTo(Point::new(0.0, 0.0)),
                PathSegment::LineTo(Point::new(1.0, 0.0)),
                PathSegment::QuadTo(Point::new(2.0, 0.0), Point::new(2.0, 1.0)),
                PathSegment::CubicTo(
                    Point::new(2.0, 2.0),
                    Point::new(1.0, 3.0),
                    Point::new(0.0, 3.0),
                ),
                PathSegment::Close,
            ],
        };
        list.push(DisplayItem::FillPath {
            path: path.clone(),
            rule: FillRule::EvenOdd,
            color: Color::BLACK,
        });
        list.push(DisplayItem::StrokePath {
            path,
            style: StrokeStyle {
                width: 2.0,
                cap: LineCap::Round,
                join: LineJoin::Round,
                miter_limit: 4.0,
            },
            color: Color::BLACK,
        });
        list.push(DisplayItem::Image {
            rect: Rect::new(0.0, 0.0, 16.0, 16.0),
            image: ImageId(3),
        });
        list.push(DisplayItem::PushScrollFrame {
            id: ScrollFrameId(1),
            clip: Rect::new(0.0, 0.0, 50.0, 50.0),
            content_size: Size::new(50.0, 500.0),
        });
        list.push(DisplayItem::Pop);
        list.push(DisplayItem::Pop);
        list.push(DisplayItem::Pop);
        list.push(DisplayItem::Pop);
        list
    }

    #[test]
    fn display_list_round_trips_through_postcard() {
        let list = sample();
        let bytes = postcard::to_allocvec(&list).unwrap();
        let back: DisplayList = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(list, back);
    }

    #[test]
    fn truncated_bytes_fail_to_decode_instead_of_panicking() {
        let bytes = postcard::to_allocvec(&sample()).unwrap();
        for len in 0..bytes.len() {
            assert!(postcard::from_bytes::<DisplayList>(&bytes[..len]).is_err());
        }
    }

    #[test]
    fn transform_then_composes_in_order() {
        let t = Transform::scale(2.0, 2.0).then(Transform::translate(10.0, 0.0));
        // scale first, then translate: (1, 1) -> (2, 2) -> (12, 2)
        let [a, b, c, d, e, f] = t.0;
        assert_eq!((a + c + e, b + d + f), (12.0, 2.0));
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(r.contains(Point::new(0.0, 0.0)));
        assert!(r.contains(Point::new(9.9, 9.9)));
        assert!(!r.contains(Point::new(10.0, 5.0)));
    }
}
