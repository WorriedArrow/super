//! lucide icons (crates/sp_ui/icons, release pinned in LUCIDE_VERSION),
//! turned from svg source into display list paths and drawn as strokes the
//! way lucide specifies them: a 24x24 viewbox, stroke-width 2 scaled with the
//! icon, round caps and joins, no fill.

use sp_compositor::kurbo::{self, BezPath, PathEl, Shape};
use sp_paint::{
    Color, DisplayItem, DisplayList, LineCap, LineJoin, Path, PathSegment, Point, StrokeStyle,
    Transform,
};

const VIEWBOX: f32 = 24.0;
const STROKE_WIDTH: f32 = 2.0;

pub(crate) struct Icon(Path);

pub(crate) struct Icons {
    pub arrow_left: Icon,
    pub arrow_right: Icon,
    pub rotate_cw: Icon,
}

impl Icons {
    pub fn load() -> Self {
        Self {
            arrow_left: Icon::parse(include_str!("../icons/arrow-left.svg")),
            arrow_right: Icon::parse(include_str!("../icons/arrow-right.svg")),
            rotate_cw: Icon::parse(include_str!("../icons/rotate-cw.svg")),
        }
    }
}

impl Icon {
    /// the shapes of a lucide svg. lucide icons use path, circle, ellipse,
    /// rect, line, polyline and polygon; anything else is skipped.
    pub fn parse(svg: &str) -> Self {
        let mut bez = BezPath::new();
        for (name, attrs) in elements(svg) {
            let num = |key: &str| attr(attrs, key).and_then(|v| v.trim().parse::<f64>().ok());
            match name {
                "path" => {
                    if let Some(d) = attr(attrs, "d").and_then(|d| BezPath::from_svg(d).ok()) {
                        bez.extend(d);
                    }
                }
                "circle" => {
                    let (Some(cx), Some(cy), Some(r)) = (num("cx"), num("cy"), num("r")) else {
                        continue;
                    };
                    bez.extend(kurbo::Circle::new((cx, cy), r).path_elements(0.01));
                }
                "ellipse" => {
                    let (Some(cx), Some(cy), Some(rx), Some(ry)) =
                        (num("cx"), num("cy"), num("rx"), num("ry"))
                    else {
                        continue;
                    };
                    bez.extend(kurbo::Ellipse::new((cx, cy), (rx, ry), 0.0).path_elements(0.01));
                }
                "rect" => {
                    let (Some(w), Some(h)) = (num("width"), num("height")) else {
                        continue;
                    };
                    let (x, y) = (num("x").unwrap_or(0.0), num("y").unwrap_or(0.0));
                    let r = num("rx").or(num("ry")).unwrap_or(0.0);
                    let rect = kurbo::Rect::new(x, y, x + w, y + h).to_rounded_rect(r);
                    bez.extend(rect.path_elements(0.01));
                }
                "line" => {
                    let (Some(x1), Some(y1), Some(x2), Some(y2)) =
                        (num("x1"), num("y1"), num("x2"), num("y2"))
                    else {
                        continue;
                    };
                    bez.move_to((x1, y1));
                    bez.line_to((x2, y2));
                }
                "polyline" | "polygon" => {
                    let Some(points) = attr(attrs, "points") else {
                        continue;
                    };
                    let coords: Vec<f64> = points
                        .split(|c: char| c == ',' || c.is_whitespace())
                        .filter_map(|v| v.parse().ok())
                        .collect();
                    for (i, pair) in coords.as_chunks::<2>().0.iter().enumerate() {
                        if i == 0 {
                            bez.move_to((pair[0], pair[1]));
                        } else {
                            bez.line_to((pair[0], pair[1]));
                        }
                    }
                    if name == "polygon" && coords.len() >= 4 {
                        bez.close_path();
                    }
                }
                _ => {}
            }
        }
        Icon(to_path(&bez))
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.segments.is_empty()
    }

    /// draws the icon `size` css pixels square with its top left at
    /// `origin`. `scale` is device pixels per css pixel: the origin and the
    /// stroke width are snapped to whole device pixels so the icon stays
    /// sharp.
    pub fn draw(&self, list: &mut DisplayList, origin: Point, size: f32, scale: f32, color: Color) {
        let snap = |v: f32| (v * scale).round() / scale;
        let k = size / VIEWBOX;
        let device_width = (STROKE_WIDTH * k * scale).round().max(1.0);
        list.push(DisplayItem::PushTransform(
            Transform::scale(k, k).then(Transform::translate(snap(origin.x), snap(origin.y))),
        ));
        list.push(DisplayItem::StrokePath {
            path: self.0.clone(),
            style: StrokeStyle {
                width: device_width / scale / k,
                cap: LineCap::Round,
                join: LineJoin::Round,
                miter_limit: 4.0,
            },
            color,
        });
        list.push(DisplayItem::Pop);
    }
}

fn to_path(bez: &BezPath) -> Path {
    let p = |p: kurbo::Point| Point::new(p.x as f32, p.y as f32);
    Path {
        segments: bez
            .elements()
            .iter()
            .map(|el| match *el {
                PathEl::MoveTo(a) => PathSegment::MoveTo(p(a)),
                PathEl::LineTo(a) => PathSegment::LineTo(p(a)),
                PathEl::QuadTo(a, b) => PathSegment::QuadTo(p(a), p(b)),
                PathEl::CurveTo(a, b, c) => PathSegment::CubicTo(p(a), p(b), p(c)),
                PathEl::ClosePath => PathSegment::Close,
            })
            .collect(),
    }
}

/// `(name, attributes)` of every element start tag in `svg`, except the
/// root `svg` element. enough xml for lucide's files: no comments, cdata or
/// entities.
fn elements(svg: &str) -> impl Iterator<Item = (&str, &str)> {
    svg.split('<').skip(1).filter_map(|tag| {
        let tag = tag.split('>').next()?.trim_end_matches('/').trim();
        if tag.starts_with(['/', '?', '!']) {
            return None;
        }
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        (name != "svg").then_some((name, attrs))
    })
}

fn attr<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let name = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let value_end = after[1..].find(quote)? + 1;
        if name == key {
            return Some(&after[1..value_end]);
        }
        rest = &after[value_end + 1..];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_icons_parse_into_paths() {
        let icons = Icons::load();
        for icon in [&icons.arrow_left, &icons.arrow_right, &icons.rotate_cw] {
            assert!(!icon.is_empty());
            assert!(matches!(icon.0.segments[0], PathSegment::MoveTo(_)));
        }
    }

    #[test]
    fn arrow_left_matches_its_svg() {
        let icon = Icon::parse(include_str!("../icons/arrow-left.svg"));
        // <path d="m12 19-7-7 7-7" /> <path d="M19 12H5" />
        let points: Vec<_> = icon
            .0
            .segments
            .iter()
            .map(|s| match s {
                PathSegment::MoveTo(p) | PathSegment::LineTo(p) => (p.x, p.y),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            points,
            vec![
                (12.0, 19.0),
                (5.0, 12.0),
                (12.0, 5.0),
                (19.0, 12.0),
                (5.0, 12.0)
            ]
        );
    }

    #[test]
    fn basic_shapes_parse() {
        let svg = r#"<svg viewBox="0 0 24 24">
            <circle cx="12" cy="12" r="10" />
            <rect width="18" height="18" x="3" y="3" rx="2" />
            <line x1="2" x2="22" y1="12" y2="12" />
            <polyline points="22 7 13.5 15.5 8.5 10.5 2 17" />
            <polygon points="1,1 2,1 2,2" />
            <ellipse cx="12" cy="5" rx="9" ry="3" />
        </svg>"#;
        let icon = Icon::parse(svg);
        let moves = icon
            .0
            .segments
            .iter()
            .filter(|s| matches!(s, PathSegment::MoveTo(_)))
            .count();
        assert_eq!(moves, 6);
    }

    #[test]
    fn attributes_are_found_by_exact_name() {
        assert_eq!(attr(r#"x1="2" x="5""#, "x"), Some("5"));
        assert_eq!(attr(r#"d='M1 2'"#, "d"), Some("M1 2"));
        assert_eq!(attr(r#"d=M1"#, "d"), None);
    }

    #[test]
    fn stroke_width_snaps_to_device_pixels() {
        let icon = Icon::parse(include_str!("../icons/arrow-left.svg"));
        for (size, scale) in [(20.0, 1.0), (20.0, 1.25), (18.0, 2.0)] {
            let mut list = DisplayList::new();
            icon.draw(&mut list, Point::new(3.3, 4.7), size, scale, Color::BLACK);
            let DisplayItem::StrokePath { style, .. } = &list.items[1] else {
                panic!("no stroke");
            };
            let device = style.width * size / VIEWBOX * scale;
            assert!((device - device.round()).abs() < 1e-4, "{device}");
        }
    }
}
