//! renders small display lists offscreen and checks pixels.
//!
//! needs a gpu adapter. where none exists the test logs why and returns
//! early; linux ci installs mesa's lavapipe so it runs there.

use sp_compositor::{Compositor, Error};
use sp_paint::{
    Color, CornerRadii, DisplayItem, DisplayList, FillRule, ImageId, LineCap, LineJoin, Path,
    PathSegment, Point, Rect, ScrollFrameId, Sides, Size, StrokeStyle, Transform,
};

const W: u32 = 64;
const H: u32 = 64;

fn compositor() -> Option<Compositor> {
    match Compositor::headless() {
        Ok(c) => {
            eprintln!("rendering on {}", c.adapter_name());
            Some(c)
        }
        Err(Error::NoAdapter(reason)) => {
            eprintln!("skipping offscreen render test: no gpu adapter ({reason})");
            None
        }
        Err(e) => panic!("compositor setup failed: {e}"),
    }
}

struct Pixels {
    data: Vec<u8>,
    width: u32,
}

impl Pixels {
    fn at(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        self.data[i..i + 4].try_into().unwrap()
    }

    #[track_caller]
    fn assert_near(&self, x: u32, y: u32, want: [u8; 4]) {
        let got = self.at(x, y);
        let close = got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2);
        assert!(close, "pixel ({x}, {y}) is {got:?}, want {want:?}");
    }
}

fn render(c: &mut Compositor, list: DisplayList) -> Pixels {
    let rect = Rect::new(0.0, 0.0, W as f32, H as f32);
    c.set_content(list, rect);
    let data = c.render_to_rgba(W, H).expect("offscreen render");
    assert_eq!(data.len(), (W * H * 4) as usize);
    Pixels { data, width: W }
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn offscreen_render_matches_display_lists() {
    let Some(mut c) = compositor() else { return };
    c.set_background(Color::WHITE);

    // rects, and the background showing through elsewhere.
    let mut list = DisplayList::new();
    list.rect(Rect::new(8.0, 8.0, 16.0, 16.0), Color::rgb(255, 0, 0));
    let px = render(&mut c, list);
    px.assert_near(16, 16, RED);
    px.assert_near(8, 8, RED);
    px.assert_near(23, 23, RED);
    px.assert_near(24, 24, WHITE);
    px.assert_near(40, 40, WHITE);

    // rounded rect: the corner pixel stays background, the middle is filled.
    let mut list = DisplayList::new();
    list.rounded_rect(
        Rect::new(0.0, 0.0, 32.0, 32.0),
        CornerRadii::uniform(16.0),
        Color::rgb(0, 0, 255),
    );
    let px = render(&mut c, list);
    px.assert_near(0, 0, WHITE);
    px.assert_near(16, 16, BLUE);

    // clip: a big rect clipped to a small one.
    let mut list = DisplayList::new();
    list.push(DisplayItem::PushClip {
        rect: Rect::new(10.0, 10.0, 10.0, 10.0),
        radii: CornerRadii::default(),
    });
    list.rect(Rect::new(0.0, 0.0, 64.0, 64.0), Color::rgb(0, 255, 0));
    list.push(DisplayItem::Pop);
    list.rect(Rect::new(40.0, 40.0, 4.0, 4.0), Color::rgb(255, 0, 0));
    let px = render(&mut c, list);
    px.assert_near(15, 15, GREEN);
    px.assert_near(5, 5, WHITE);
    px.assert_near(30, 30, WHITE);
    // the pop ended the clip.
    px.assert_near(42, 42, RED);

    // transform: translate then scale.
    let mut list = DisplayList::new();
    list.push(DisplayItem::PushTransform(
        Transform::scale(2.0, 2.0).then(Transform::translate(32.0, 0.0)),
    ));
    list.rect(Rect::new(0.0, 0.0, 8.0, 8.0), Color::rgb(0, 0, 255));
    list.push(DisplayItem::Pop);
    let px = render(&mut c, list);
    px.assert_near(40, 8, BLUE);
    px.assert_near(47, 15, BLUE);
    px.assert_near(49, 17, WHITE);
    px.assert_near(8, 8, WHITE);

    // opacity: 50% black over white is mid gray.
    let mut list = DisplayList::new();
    list.push(DisplayItem::PushOpacity(0.5));
    list.rect(Rect::new(0.0, 0.0, 32.0, 32.0), Color::BLACK);
    list.push(DisplayItem::Pop);
    let px = render(&mut c, list);
    let [r, g, b, a] = px.at(16, 16);
    assert!(
        (120..=136).contains(&r) && r == g && g == b && a == 255,
        "{:?}",
        px.at(16, 16)
    );
    px.assert_near(48, 48, WHITE);

    // border with a different color per side.
    let mut list = DisplayList::new();
    list.push(DisplayItem::Border {
        rect: Rect::new(0.0, 0.0, 64.0, 64.0),
        radii: CornerRadii::default(),
        widths: Sides::all(8.0),
        colors: Sides {
            top: Color::rgb(255, 0, 0),
            right: Color::rgb(0, 255, 0),
            bottom: Color::rgb(0, 0, 255),
            left: Color::BLACK,
        },
    });
    let px = render(&mut c, list);
    px.assert_near(32, 3, RED);
    px.assert_near(60, 32, GREEN);
    px.assert_near(32, 60, BLUE);
    px.assert_near(3, 32, [0, 0, 0, 255]);
    px.assert_near(32, 32, WHITE);

    // image: a 2x2 image scaled up to fill the frame.
    #[rustfmt::skip]
    let pixels = vec![
        255, 0, 0, 255,   0, 255, 0, 255,
        0, 0, 255, 255,   0, 0, 0, 255,
    ];
    c.resources_mut()
        .add_image(ImageId(1), 2, 2, pixels)
        .expect("valid image");
    let mut list = DisplayList::new();
    list.push(DisplayItem::Image {
        rect: Rect::new(0.0, 0.0, 64.0, 64.0),
        image: ImageId(1),
    });
    let px = render(&mut c, list);
    px.assert_near(8, 8, RED);
    px.assert_near(56, 8, GREEN);
    px.assert_near(8, 56, BLUE);
    px.assert_near(56, 56, [0, 0, 0, 255]);

    // paths: a filled triangle and a stroked line with round caps.
    let mut list = DisplayList::new();
    list.push(DisplayItem::FillPath {
        path: Path {
            segments: vec![
                PathSegment::MoveTo(Point::new(0.0, 0.0)),
                PathSegment::LineTo(Point::new(32.0, 0.0)),
                PathSegment::LineTo(Point::new(0.0, 32.0)),
                PathSegment::Close,
            ],
        },
        rule: FillRule::NonZero,
        color: Color::rgb(0, 0, 255),
    });
    list.push(DisplayItem::StrokePath {
        path: Path {
            segments: vec![
                PathSegment::MoveTo(Point::new(16.0, 48.0)),
                PathSegment::LineTo(Point::new(48.0, 48.0)),
            ],
        },
        style: StrokeStyle {
            width: 8.0,
            cap: LineCap::Round,
            join: LineJoin::Round,
            miter_limit: 4.0,
        },
        color: Color::rgb(255, 0, 0),
    });
    let px = render(&mut c, list);
    px.assert_near(4, 4, BLUE);
    px.assert_near(28, 28, WHITE);
    px.assert_near(32, 48, RED);
    px.assert_near(13, 48, RED);
    px.assert_near(32, 55, WHITE);
    px.assert_near(8, 48, WHITE);

    // scroll frame: clipped, and moved by the compositor's offset.
    let mut list = DisplayList::new();
    list.push(DisplayItem::PushScrollFrame {
        id: ScrollFrameId(1),
        clip: Rect::new(0.0, 0.0, 32.0, 32.0),
        content_size: Size::new(32.0, 64.0),
    });
    list.rect(Rect::new(0.0, 16.0, 64.0, 8.0), Color::rgb(255, 0, 0));
    list.push(DisplayItem::Pop);
    c.set_scroll_offset(ScrollFrameId(1), 0.0, 10.0);
    let px = render(&mut c, list);
    px.assert_near(16, 10, RED);
    px.assert_near(16, 20, WHITE);
    px.assert_near(40, 10, WHITE);

    // content is clipped to the content rect and offset by its origin, and
    // the chrome draws on top in window coordinates. scale doubles both.
    let mut content = DisplayList::new();
    content.rect(Rect::new(0.0, 0.0, 100.0, 100.0), Color::rgb(0, 0, 255));
    c.set_content(content, Rect::new(0.0, 8.0, 24.0, 20.0));
    let mut chrome = DisplayList::new();
    chrome.rect(Rect::new(0.0, 0.0, 32.0, 8.0), Color::rgb(255, 0, 0));
    c.set_chrome(chrome);
    c.set_scale(2.0);
    let px = Pixels {
        data: c.render_to_rgba(W, H).unwrap(),
        width: W,
    };
    px.assert_near(32, 8, RED);
    px.assert_near(32, 40, BLUE);
    px.assert_near(10, 20, BLUE);
    px.assert_near(56, 40, WHITE);
    px.assert_near(32, 60, WHITE);
}

#[test]
fn invalid_resources_are_rejected() {
    let mut r = sp_compositor::Resources::default();
    assert!(r.add_image(ImageId(1), 2, 2, vec![0; 15]).is_err());
    assert!(r.add_image(ImageId(1), 0, 0, vec![]).is_err());
    assert!(r.add_image(ImageId(1), 1, 1, vec![0; 4]).is_ok());
    let not_a_font = sp_compositor::Blob::from(vec![0u8; 64]);
    assert!(r.add_font(sp_paint::FontId(1), not_a_font, 0).is_err());
}
