//! turns display lists into a vello scene.
//!
//! display lists come from renderers, which are untrusted, so nothing here
//! may panic on malformed input: unbalanced pops are ignored, unclosed pushes
//! are closed at the end of the list, non-finite geometry is skipped and
//! unknown font or image ids draw nothing.

use std::collections::HashMap;

use sp_paint::{
    Color, CornerRadii, DisplayItem, DisplayList, FillRule, GlyphRun, ImageId, LineCap, LineJoin,
    Path, PathSegment, Rect, ScrollFrameId, Sides, StrokeStyle, Transform,
};
use vello::Scene;
use vello::kurbo::{self, Affine, BezPath, RoundedRectRadii, Shape};
use vello::peniko::{self, BlendMode, Fill};

use crate::Resources;

pub(crate) struct SceneBuilder<'a> {
    pub scene: &'a mut Scene,
    pub resources: &'a Resources,
    pub scroll_offsets: &'a HashMap<ScrollFrameId, (f32, f32)>,
    /// the whole target in device pixels, used as the clip shape of opacity
    /// groups, which have no clip of their own.
    pub viewport: kurbo::Rect,
}

enum Pushed {
    /// a vello layer (clip, opacity or scroll frame) plus the transform to
    /// restore.
    Layer(Affine),
    Transform(Affine),
}

impl SceneBuilder<'_> {
    /// appends `list`, with every item transformed by `base` and clipped to
    /// `clip` (in the coordinate space of `base`) if given.
    pub fn append(&mut self, list: &DisplayList, base: Affine, clip: Option<Rect>) {
        if let Some(clip) = clip {
            let Some(clip) = to_rect(clip) else { return };
            self.push_clip(base, &clip);
        }
        let mut t = base;
        let mut stack: Vec<Pushed> = Vec::new();
        for item in &list.items {
            match item {
                DisplayItem::Rect { rect, color } => {
                    if let Some(rect) = to_rect(*rect) {
                        self.scene
                            .fill(Fill::NonZero, t, to_color(*color), None, &rect);
                    }
                }
                DisplayItem::RoundedRect { rect, radii, color } => {
                    if let Some(shape) = rounded_rect(*rect, *radii) {
                        self.scene
                            .fill(Fill::NonZero, t, to_color(*color), None, &shape);
                    }
                }
                DisplayItem::Border {
                    rect,
                    radii,
                    widths,
                    colors,
                } => self.border(t, *rect, *radii, *widths, *colors),
                DisplayItem::GlyphRun(run) => self.glyph_run(t, run),
                DisplayItem::FillPath { path, rule, color } => {
                    if let Some(path) = to_bez_path(path) {
                        let rule = match rule {
                            FillRule::NonZero => Fill::NonZero,
                            FillRule::EvenOdd => Fill::EvenOdd,
                        };
                        self.scene.fill(rule, t, to_color(*color), None, &path);
                    }
                }
                DisplayItem::StrokePath { path, style, color } => {
                    if let (Some(path), Some(stroke)) = (to_bez_path(path), to_stroke(style)) {
                        self.scene.stroke(&stroke, t, to_color(*color), None, &path);
                    }
                }
                DisplayItem::Image { rect, image } => self.image(t, *rect, *image),
                DisplayItem::PushClip { rect, radii } => {
                    match rounded_rect(*rect, *radii) {
                        Some(shape) => self.push_clip(t, &shape),
                        // an invalid clip rect clips everything away.
                        None => self.push_clip(t, &kurbo::Rect::ZERO),
                    }
                    stack.push(Pushed::Layer(t));
                }
                DisplayItem::PushTransform(transform) => {
                    stack.push(Pushed::Transform(t));
                    t *= to_affine(*transform);
                }
                DisplayItem::PushOpacity(alpha) => {
                    let alpha = if alpha.is_nan() {
                        0.0
                    } else {
                        alpha.clamp(0.0, 1.0)
                    };
                    self.scene.push_layer(
                        Fill::NonZero,
                        BlendMode::default(),
                        alpha,
                        Affine::IDENTITY,
                        &self.viewport,
                    );
                    stack.push(Pushed::Layer(t));
                }
                DisplayItem::PushScrollFrame { id, clip, .. } => {
                    match to_rect(*clip) {
                        Some(clip) => self.push_clip(t, &clip),
                        None => self.push_clip(t, &kurbo::Rect::ZERO),
                    }
                    stack.push(Pushed::Layer(t));
                    let (dx, dy) = self.scroll_offsets.get(id).copied().unwrap_or_default();
                    if dx.is_finite() && dy.is_finite() {
                        t *= Affine::translate((-f64::from(dx), -f64::from(dy)));
                    }
                }
                DisplayItem::Pop => match stack.pop() {
                    Some(Pushed::Layer(prev)) => {
                        self.scene.pop_layer();
                        t = prev;
                    }
                    Some(Pushed::Transform(prev)) => t = prev,
                    None => {}
                },
            }
        }
        for pushed in stack.into_iter().rev() {
            if let Pushed::Layer(_) = pushed {
                self.scene.pop_layer();
            }
        }
        if clip.is_some() {
            self.scene.pop_layer();
        }
    }

    // a full layer rather than vello's push_clip_layer, since clip layers
    // don't isolate blending yet (vello issue 1198) and opacity groups nest
    // inside clips.
    fn push_clip(&mut self, t: Affine, shape: &impl Shape) {
        self.scene
            .push_layer(Fill::NonZero, BlendMode::default(), 1.0, t, shape);
    }

    fn glyph_run(&mut self, t: Affine, run: &GlyphRun) {
        let Some(font) = self.resources.fonts.get(&run.font) else {
            return;
        };
        if !(run.size.is_finite() && run.size > 0.0) {
            return;
        }
        self.scene
            .draw_glyphs(font)
            .font_size(run.size)
            // vello drops hinting itself when the transform isn't a uniform
            // scale plus translation.
            .hint(true)
            .transform(t)
            .brush(to_color(run.color))
            .draw(
                Fill::NonZero,
                run.glyphs
                    .iter()
                    .filter(|g| g.position.x.is_finite() && g.position.y.is_finite())
                    .map(|g| vello::Glyph {
                        id: g.id,
                        x: g.position.x,
                        y: g.position.y,
                    }),
            );
    }

    fn image(&mut self, t: Affine, rect: Rect, id: ImageId) {
        let Some(image) = self.resources.images.get(&id) else {
            return;
        };
        let Some(dest) = to_rect(rect) else { return };
        let sx = dest.width() / f64::from(image.width);
        let sy = dest.height() / f64::from(image.height);
        let placement = Affine::translate((dest.x0, dest.y0)) * Affine::scale_non_uniform(sx, sy);
        self.scene.draw_image(image, t * placement);
    }

    // https://drafts.csswg.org/css-backgrounds-3/#corner-shaping
    // the padding edge's radii are the border edge's radii minus the adjacent
    // border widths. kurbo corners are circular, so each inner corner uses the
    // smaller of its two reduced radii.
    fn border(
        &mut self,
        t: Affine,
        rect: Rect,
        radii: CornerRadii,
        widths: Sides<f32>,
        colors: Sides<Color>,
    ) {
        let Some(outer_rect) = to_rect(rect) else {
            return;
        };
        let w = |v: f32| {
            if v.is_finite() {
                f64::from(v.max(0.0))
            } else {
                0.0
            }
        };
        let (top, right, bottom, left) = (
            w(widths.top),
            w(widths.right),
            w(widths.bottom),
            w(widths.left),
        );
        let outer_radii = fit_radii(outer_rect, radii);
        let inner_rect = kurbo::Rect::new(
            outer_rect.x0 + left,
            outer_rect.y0 + top,
            (outer_rect.x1 - right).max(outer_rect.x0 + left),
            (outer_rect.y1 - bottom).max(outer_rect.y0 + top),
        );
        let inner_radii = RoundedRectRadii::new(
            (outer_radii.top_left - left.max(top)).max(0.0),
            (outer_radii.top_right - right.max(top)).max(0.0),
            (outer_radii.bottom_right - right.max(bottom)).max(0.0),
            (outer_radii.bottom_left - left.max(bottom)).max(0.0),
        );
        let mut ring = BezPath::new();
        ring.extend(kurbo::RoundedRect::from_rect(outer_rect, outer_radii).path_elements(0.1));
        ring.extend(kurbo::RoundedRect::from_rect(inner_rect, inner_radii).path_elements(0.1));

        let all_same =
            colors.top == colors.right && colors.top == colors.bottom && colors.top == colors.left;
        if all_same {
            self.scene
                .fill(Fill::EvenOdd, t, to_color(colors.top), None, &ring);
            return;
        }
        // with differing colors each side owns the trapezoid between its
        // outer edge, its inner edge and the corner diagonals.
        let (o, i) = (outer_rect, inner_rect);
        let sides = [
            (
                top,
                colors.top,
                [(o.x0, o.y0), (o.x1, o.y0), (i.x1, i.y0), (i.x0, i.y0)],
            ),
            (
                right,
                colors.right,
                [(o.x1, o.y0), (o.x1, o.y1), (i.x1, i.y1), (i.x1, i.y0)],
            ),
            (
                bottom,
                colors.bottom,
                [(o.x1, o.y1), (o.x0, o.y1), (i.x0, i.y1), (i.x1, i.y1)],
            ),
            (
                left,
                colors.left,
                [(o.x0, o.y1), (o.x0, o.y0), (i.x0, i.y0), (i.x0, i.y1)],
            ),
        ];
        for (width, color, quad) in sides {
            if width <= 0.0 || color.a == 0 {
                continue;
            }
            let mut clip = BezPath::new();
            clip.move_to(quad[0]);
            for p in &quad[1..] {
                clip.line_to(*p);
            }
            clip.close_path();
            self.push_clip(t, &clip);
            self.scene
                .fill(Fill::EvenOdd, t, to_color(color), None, &ring);
            self.scene.pop_layer();
        }
    }
}

/// None if any coordinate is non-finite. a segment that needs a current
/// point but has none starts a subpath at its first point instead, which is
/// what kurbo requires.
fn to_bez_path(path: &Path) -> Option<BezPath> {
    let p = |p: sp_paint::Point| {
        (p.x.is_finite() && p.y.is_finite())
            .then(|| kurbo::Point::new(f64::from(p.x), f64::from(p.y)))
    };
    let mut out = BezPath::new();
    let mut open = false;
    for seg in &path.segments {
        let first = match seg {
            PathSegment::MoveTo(a) | PathSegment::LineTo(a) => *a,
            PathSegment::QuadTo(a, _) | PathSegment::CubicTo(a, _, _) => *a,
            PathSegment::Close => {
                if open {
                    out.close_path();
                    open = false;
                }
                continue;
            }
        };
        if !open && !matches!(seg, PathSegment::MoveTo(_)) {
            out.move_to(p(first)?);
        }
        match *seg {
            PathSegment::MoveTo(a) => out.move_to(p(a)?),
            PathSegment::LineTo(a) => out.line_to(p(a)?),
            PathSegment::QuadTo(a, b) => out.quad_to(p(a)?, p(b)?),
            PathSegment::CubicTo(a, b, c) => out.curve_to(p(a)?, p(b)?, p(c)?),
            PathSegment::Close => {}
        }
        open = true;
    }
    Some(out)
}

fn to_stroke(style: &StrokeStyle) -> Option<kurbo::Stroke> {
    if !(style.width.is_finite() && style.width > 0.0) {
        return None;
    }
    let miter = if style.miter_limit.is_finite() {
        f64::from(style.miter_limit.max(1.0))
    } else {
        4.0
    };
    Some(
        kurbo::Stroke::new(f64::from(style.width))
            .with_caps(match style.cap {
                LineCap::Butt => kurbo::Cap::Butt,
                LineCap::Round => kurbo::Cap::Round,
                LineCap::Square => kurbo::Cap::Square,
            })
            .with_join(match style.join {
                LineJoin::Miter => kurbo::Join::Miter,
                LineJoin::Round => kurbo::Join::Round,
                LineJoin::Bevel => kurbo::Join::Bevel,
            })
            .with_miter_limit(miter),
    )
}

pub(crate) fn to_color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn to_affine(t: Transform) -> Affine {
    if t.0.iter().all(|v| v.is_finite()) {
        Affine::new(t.0.map(f64::from))
    } else {
        // css never produces a non-finite matrix; treat a corrupt one as
        // collapsing its contents to nothing.
        Affine::scale(0.0)
    }
}

/// None for non-finite or negative-size rects.
pub(crate) fn to_rect(r: Rect) -> Option<kurbo::Rect> {
    let Rect { origin, size } = r;
    let ok = [origin.x, origin.y, size.width, size.height]
        .iter()
        .all(|v| v.is_finite())
        && size.width >= 0.0
        && size.height >= 0.0;
    ok.then(|| {
        kurbo::Rect::new(
            f64::from(origin.x),
            f64::from(origin.y),
            f64::from(origin.x + size.width),
            f64::from(origin.y + size.height),
        )
    })
}

fn rounded_rect(rect: Rect, radii: CornerRadii) -> Option<kurbo::RoundedRect> {
    let rect = to_rect(rect)?;
    Some(kurbo::RoundedRect::from_rect(rect, fit_radii(rect, radii)))
}

// https://drafts.csswg.org/css-backgrounds-3/#corner-overlap
// "let f = min(Li/Si), where i ∈ {top, right, bottom, left}, Si is the sum of
// the two corresponding radii of the corners on side i, and Li is the length
// of side i of the box. if f < 1, then all corner radii are reduced by
// multiplying them by f."
pub(crate) fn fit_radii(rect: kurbo::Rect, radii: CornerRadii) -> RoundedRectRadii {
    let r = |v: f32| {
        if v.is_finite() {
            f64::from(v.max(0.0))
        } else {
            0.0
        }
    };
    let (tl, tr, br, bl) = (
        r(radii.top_left),
        r(radii.top_right),
        r(radii.bottom_right),
        r(radii.bottom_left),
    );
    let (w, h) = (rect.width(), rect.height());
    let mut f: f64 = 1.0;
    for (len, sum) in [(w, tl + tr), (h, tr + br), (w, br + bl), (h, bl + tl)] {
        if sum > 0.0 {
            f = f.min(len / sum);
        }
    }
    let f = f.max(0.0);
    RoundedRectRadii::new(tl * f, tr * f, br * f, bl * f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_radii_scale_down_uniformly() {
        let rect = kurbo::Rect::new(0.0, 0.0, 100.0, 40.0);
        let fitted = fit_radii(rect, CornerRadii::uniform(30.0));
        // height 40 / (30 + 30)
        let f = 40.0 / 60.0;
        assert!((fitted.top_left - 30.0 * f).abs() < 1e-9);
        assert!((fitted.bottom_right - 30.0 * f).abs() < 1e-9);
    }

    #[test]
    fn radii_that_fit_are_unchanged() {
        let rect = kurbo::Rect::new(0.0, 0.0, 100.0, 40.0);
        let fitted = fit_radii(rect, CornerRadii::uniform(10.0));
        assert_eq!(fitted.top_left, 10.0);
    }

    #[test]
    fn paths_without_a_move_get_one() {
        let path = Path {
            segments: vec![
                PathSegment::LineTo(sp_paint::Point::new(1.0, 2.0)),
                PathSegment::LineTo(sp_paint::Point::new(3.0, 4.0)),
                PathSegment::Close,
                PathSegment::LineTo(sp_paint::Point::new(5.0, 6.0)),
            ],
        };
        let bez = to_bez_path(&path).unwrap();
        let moves = bez
            .elements()
            .iter()
            .filter(|e| matches!(e, kurbo::PathEl::MoveTo(_)))
            .count();
        assert_eq!(moves, 2);
        let bad = Path {
            segments: vec![PathSegment::MoveTo(sp_paint::Point::new(f32::NAN, 0.0))],
        };
        assert!(to_bez_path(&bad).is_none());
    }

    #[test]
    fn bad_rects_are_rejected() {
        assert!(to_rect(Rect::new(f32::NAN, 0.0, 1.0, 1.0)).is_none());
        assert!(to_rect(Rect::new(0.0, 0.0, -1.0, 1.0)).is_none());
        assert!(to_rect(Rect::new(0.0, 0.0, f32::INFINITY, 1.0)).is_none());
        assert!(to_rect(Rect::new(0.0, 0.0, 0.0, 0.0)).is_some());
    }

    #[test]
    fn malformed_lists_build_without_panicking() {
        let mut scene = Scene::new();
        let resources = Resources::default();
        let offsets = HashMap::new();
        let mut builder = SceneBuilder {
            scene: &mut scene,
            resources: &resources,
            scroll_offsets: &offsets,
            viewport: kurbo::Rect::new(0.0, 0.0, 10.0, 10.0),
        };
        let mut list = DisplayList::new();
        list.push(DisplayItem::Pop);
        list.push(DisplayItem::PushOpacity(f32::NAN));
        list.push(DisplayItem::PushTransform(Transform([f32::NAN; 6])));
        list.push(DisplayItem::PushClip {
            rect: Rect::new(0.0, 0.0, -5.0, 1.0),
            radii: CornerRadii::uniform(f32::INFINITY),
        });
        list.push(DisplayItem::GlyphRun(GlyphRun {
            font: sp_paint::FontId(99),
            size: 12.0,
            color: Color::BLACK,
            glyphs: vec![],
        }));
        list.push(DisplayItem::Image {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            image: ImageId(99),
        });
        list.push(DisplayItem::Border {
            rect: Rect::new(0.0, 0.0, 4.0, 4.0),
            radii: CornerRadii::uniform(100.0),
            widths: Sides::all(f32::NAN),
            colors: Sides::all(Color::BLACK),
        });
        list.push(DisplayItem::StrokePath {
            path: Path {
                segments: vec![
                    PathSegment::Close,
                    PathSegment::LineTo(sp_paint::Point::new(1.0, 1.0)),
                    PathSegment::Close,
                    PathSegment::CubicTo(
                        sp_paint::Point::new(1.0, 1.0),
                        sp_paint::Point::new(f32::NAN, 1.0),
                        sp_paint::Point::new(1.0, 1.0),
                    ),
                ],
            },
            style: StrokeStyle {
                width: f32::INFINITY,
                ..StrokeStyle::default()
            },
            color: Color::BLACK,
        });
        builder.append(&list, Affine::IDENTITY, Some(Rect::new(0.0, 0.0, 5.0, 5.0)));
        assert_eq!(scene.encoding().n_open_clips, 0);
    }
}
