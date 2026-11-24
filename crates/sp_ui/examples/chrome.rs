//! opens the browser window with a hard-coded demo display list in the
//! content area. `cargo run -p sp_ui --example chrome`

use sp_paint::{
    Color, CornerRadii, DisplayItem, DisplayList, ImageId, Point, Rect, Sides, Size, Transform,
};
use sp_ui::{Callbacks, Ui, UiProxy};

const CHECKER: ImageId = ImageId(1);

fn checkerboard() -> Vec<u8> {
    let mut px = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let c = if (x + y) % 2 == 0 {
                [0x1a, 0x73, 0xe8, 255]
            } else {
                [255, 255, 255, 255]
            };
            px.extend_from_slice(&c);
        }
    }
    px
}

fn demo(ui: &Ui) -> DisplayList {
    let mut list = DisplayList::new();
    let text = |list: &mut DisplayList, s: &str, size: f32, color: Color, x: f32, y: f32| {
        if let Some(run) = ui.shape_text(s, size, color, Point::new(x, y)) {
            list.push(DisplayItem::GlyphRun(run));
        }
    };
    text(
        &mut list,
        "Super Browser",
        32.0,
        Color::rgb(0x20, 0x21, 0x24),
        40.0,
        70.0,
    );
    text(
        &mut list,
        "a hard-coded display list drawn by the compositor under the chrome",
        16.0,
        Color::rgb(0x5f, 0x63, 0x68),
        40.0,
        100.0,
    );

    list.rect(
        Rect::new(40.0, 130.0, 120.0, 80.0),
        Color::rgb(0xea, 0x43, 0x35),
    );
    list.rounded_rect(
        Rect::new(180.0, 130.0, 120.0, 80.0),
        CornerRadii::uniform(16.0),
        Color::rgb(0x34, 0xa8, 0x53),
    );
    list.push(DisplayItem::Border {
        rect: Rect::new(320.0, 130.0, 120.0, 80.0),
        radii: CornerRadii::uniform(10.0),
        widths: Sides::all(6.0),
        colors: Sides {
            top: Color::rgb(0xea, 0x43, 0x35),
            right: Color::rgb(0xfb, 0xbc, 0x04),
            bottom: Color::rgb(0x34, 0xa8, 0x53),
            left: Color::rgb(0x1a, 0x73, 0xe8),
        },
    });
    list.push(DisplayItem::Image {
        rect: Rect::new(460.0, 130.0, 80.0, 80.0),
        image: CHECKER,
    });

    // opacity group over a stripe.
    list.rect(Rect::new(40.0, 240.0, 500.0, 20.0), Color::BLACK);
    list.push(DisplayItem::PushOpacity(0.5));
    list.rect(
        Rect::new(60.0, 230.0, 80.0, 40.0),
        Color::rgb(0x1a, 0x73, 0xe8),
    );
    list.push(DisplayItem::Pop);

    // rotated text and a rounded clip.
    list.push(DisplayItem::PushTransform(
        Transform::rotate(-0.15).then(Transform::translate(180.0, 270.0)),
    ));
    text(
        &mut list,
        "transformed",
        20.0,
        Color::rgb(0xa1, 0x42, 0xf4),
        0.0,
        0.0,
    );
    list.push(DisplayItem::Pop);
    list.push(DisplayItem::PushClip {
        rect: Rect::new(360.0, 230.0, 180.0, 40.0),
        radii: CornerRadii::uniform(20.0),
    });
    for i in 0..10 {
        let shade = 40 + i * 20;
        list.rect(
            Rect::new(360.0 + i as f32 * 18.0, 230.0, 18.0, 40.0),
            Color::rgb(shade, shade, 255),
        );
    }
    list.push(DisplayItem::Pop);
    list
}

struct Demo {
    proxy: UiProxy,
    history: Vec<String>,
    index: usize,
}

impl Demo {
    fn show(&self) {
        let url = &self.history[self.index];
        println!("showing {url}");
        self.proxy.set_url(url.clone());
        self.proxy.set_tab_title(0, url.clone());
        self.proxy
            .set_navigation_state(self.index > 0, self.index + 1 < self.history.len());
    }
}

impl Callbacks for Demo {
    fn navigate(&mut self, input: &str) {
        println!("navigate: {input}");
        self.history.truncate(self.index + 1);
        self.history.push(input.to_owned());
        self.index += 1;
        self.show();
    }

    fn back(&mut self) {
        println!("back");
        self.index = self.index.saturating_sub(1);
        self.show();
    }

    fn forward(&mut self) {
        println!("forward");
        self.index = (self.index + 1).min(self.history.len() - 1);
        self.show();
    }

    fn reload(&mut self) {
        println!("reload");
    }

    fn select_tab(&mut self, index: usize) {
        println!("select tab {index}");
    }

    fn viewport_resized(&mut self, size: Size) {
        println!("viewport {}x{}", size.width, size.height);
    }
}

fn main() {
    let ui = Ui::new().expect("event loop");
    let proxy = ui.proxy();
    proxy.add_image(CHECKER, 8, 8, checkerboard());
    proxy.set_content(demo(&ui));
    proxy.set_tabs(vec!["super://newtab".into(), "Example Domain".into()], 0);
    let demo = Demo {
        proxy: proxy.clone(),
        history: vec!["super://newtab".into()],
        index: 0,
    };
    demo.show();
    if let Err(e) = ui.run(demo) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
