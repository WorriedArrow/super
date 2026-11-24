//! the browser chrome from docs/design/ui.md: tab strip, back, forward and
//! reload buttons, and the url bar. layout, hit testing, input and focus
//! state, and painting into a display list, all in css pixels with edges
//! snapped to device pixels.

use sp_paint::{Color, CornerRadii, DisplayItem, DisplayList, FontId, Point, Rect, Sides};

use crate::field::TextField;
use crate::icons::{Icon, Icons};
use crate::text::{ChromeFont, ShapedLine};

// spacing is on a 4px grid.
pub(crate) const TAB_STRIP_HEIGHT: f32 = 40.0;
pub(crate) const TOOLBAR_HEIGHT: f32 = 44.0;
/// tab strip, toolbar and the one-pixel line under it.
pub(crate) const CHROME_HEIGHT: f32 = TAB_STRIP_HEIGHT + TOOLBAR_HEIGHT + 1.0;

const EDGE: f32 = 8.0;
const GAP: f32 = 4.0;
const TAB_MAX_WIDTH: f32 = 240.0;
const TAB_HEIGHT: f32 = 32.0;
const TAB_PADDING: f32 = 12.0;
const BUTTON_SIZE: f32 = 32.0;
const ICON_SIZE: f32 = 20.0;
const URL_HEIGHT: f32 = 32.0;
const URL_PADDING: f32 = 14.0;
const TAB_FONT_SIZE: f32 = 12.0;
const URL_FONT_SIZE: f32 = 14.0;
const FOCUS_RING_WIDTH: f32 = 2.0;

// a neutral gray palette with one accent for focus.
const STRIP_BG: Color = Color::rgb(0xeb, 0xeb, 0xeb);
const TOOLBAR_BG: Color = Color::rgb(0xff, 0xff, 0xff);
const TAB_HOVER_BG: Color = Color::rgb(0xf5, 0xf5, 0xf5);
const TAB_PRESSED_BG: Color = Color::rgb(0xdf, 0xdf, 0xdf);
const SEPARATOR: Color = Color::rgb(0xd6, 0xd6, 0xd6);
const TEXT: Color = Color::rgb(0x1f, 0x1f, 0x1f);
const MUTED_TEXT: Color = Color::rgb(0x5c, 0x5c, 0x5c);
const ICON: Color = Color::rgb(0x33, 0x33, 0x33);
const ICON_DISABLED: Color = Color::rgb(0xb8, 0xb8, 0xb8);
const BUTTON_HOVER: Color = Color::rgb(0xf0, 0xf0, 0xf0);
const BUTTON_PRESSED: Color = Color::rgb(0xe0, 0xe0, 0xe0);
const URL_BG: Color = Color::rgb(0xf2, 0xf2, 0xf2);
const URL_HOVER_BG: Color = Color::rgb(0xea, 0xea, 0xea);
const FOCUS_RING: Color = Color::rgb(0x25, 0x63, 0xeb);
const SELECTION: Color = Color::rgb(0xcf, 0xe0, 0xfc);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Hit {
    Tab(usize),
    Back,
    Forward,
    Reload,
    UrlBar,
    Content,
    Nothing,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Navigate(String),
    Back,
    Forward,
    Reload,
    SelectTab(usize),
}

const BUTTONS: [Hit; 3] = [Hit::Back, Hit::Forward, Hit::Reload];

pub(crate) struct Chrome {
    pub tabs: Vec<String>,
    pub active_tab: usize,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// the url of the active page, which the url bar shows when not editing.
    committed_url: String,
    url: TextField,
    /// the control that receives keys.
    focus: Option<Hit>,
    /// focus moved by keyboard, so non-text controls show a focus ring.
    focus_visible: bool,
    /// a drag that started in the url bar is selecting text.
    selecting: bool,
    hover: Hit,
    pressed: Option<Hit>,
    size: (f32, f32),
    /// device pixels per css pixel.
    scale: f32,
    /// horizontal scroll of the url text, to keep the caret visible.
    url_scroll: f32,
    /// the url text as last painted, for hit testing.
    url_line: Option<ShapedLine>,
}

impl Chrome {
    pub fn new() -> Self {
        Self {
            tabs: vec![String::new()],
            active_tab: 0,
            can_go_back: false,
            can_go_forward: false,
            committed_url: String::new(),
            url: TextField::default(),
            focus: None,
            focus_visible: false,
            selecting: false,
            hover: Hit::Nothing,
            pressed: None,
            size: (0.0, 0.0),
            scale: 1.0,
            url_scroll: 0.0,
            url_line: None,
        }
    }

    pub fn set_size(&mut self, width: f32, height: f32, scale: f32) {
        self.size = (width, height);
        self.scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
    }

    pub fn set_url(&mut self, url: &str) {
        self.committed_url = url.to_owned();
        if !self.url_focused() {
            self.url.set_text(url);
            self.url_scroll = 0.0;
        }
    }

    pub fn url_focused(&self) -> bool {
        self.focus == Some(Hit::UrlBar)
    }

    pub fn url_field(&mut self) -> &mut TextField {
        &mut self.url
    }

    pub fn content_rect(&self) -> Rect {
        let (w, h) = self.size;
        Rect::new(0.0, CHROME_HEIGHT, w, (h - CHROME_HEIGHT).max(0.0))
    }

    fn tab_rect(&self, i: usize) -> Rect {
        let n = self.tabs.len().max(1) as f32;
        let width = ((self.size.0 - 2.0 * EDGE) / n)
            .clamp(0.0, TAB_MAX_WIDTH)
            .floor();
        Rect::new(
            EDGE + i as f32 * width,
            TAB_STRIP_HEIGHT - TAB_HEIGHT,
            width,
            TAB_HEIGHT,
        )
    }

    fn button_rect(&self, slot: usize) -> Rect {
        Rect::new(
            EDGE + slot as f32 * (BUTTON_SIZE + GAP),
            TAB_STRIP_HEIGHT + (TOOLBAR_HEIGHT - BUTTON_SIZE) / 2.0,
            BUTTON_SIZE,
            BUTTON_SIZE,
        )
    }

    fn url_rect(&self) -> Rect {
        let x = EDGE + 3.0 * (BUTTON_SIZE + GAP) + GAP;
        Rect::new(
            x,
            TAB_STRIP_HEIGHT + (TOOLBAR_HEIGHT - URL_HEIGHT) / 2.0,
            (self.size.0 - x - EDGE).max(0.0),
            URL_HEIGHT,
        )
    }

    fn url_text_x(&self) -> f32 {
        self.url_rect().min_x() + URL_PADDING - self.url_scroll
    }

    pub fn hit(&self, p: Point) -> Hit {
        if p.y >= CHROME_HEIGHT {
            return Hit::Content;
        }
        if let Some(i) = (0..self.tabs.len()).find(|&i| self.tab_rect(i).contains(p)) {
            return Hit::Tab(i);
        }
        for (slot, hit) in BUTTONS.into_iter().enumerate() {
            if self.button_rect(slot).contains(p) {
                return hit;
            }
        }
        if self.url_rect().contains(p) {
            return Hit::UrlBar;
        }
        Hit::Nothing
    }

    fn enabled(&self, hit: Hit) -> bool {
        match hit {
            Hit::Back => self.can_go_back,
            Hit::Forward => self.can_go_forward,
            _ => true,
        }
    }

    fn url_hit(&self, x: f32) -> usize {
        self.url_line
            .as_ref()
            .map_or(self.url.text().len(), |line| {
                line.hit(x - self.url_text_x())
            })
    }

    /// returns whether anything visible changed.
    pub fn pointer_move(&mut self, p: Point) -> bool {
        let hit = self.hit(p);
        let mut changed = hit != self.hover;
        self.hover = hit;
        if self.selecting {
            let before = self.url.selection();
            let pos = self.url_hit(p.x);
            self.url.set_caret(pos, true);
            changed |= self.url.selection() != before;
        }
        changed
    }

    pub fn pointer_left(&mut self) -> bool {
        let changed = self.hover != Hit::Nothing;
        self.hover = Hit::Nothing;
        changed
    }

    pub fn pointer_down(&mut self, p: Point) -> Option<Action> {
        let hit = self.hit(p);
        self.pressed = Some(hit);
        self.focus_visible = false;
        match hit {
            Hit::UrlBar => {
                if self.url_focused() {
                    let pos = self.url_hit(p.x);
                    self.url.set_caret(pos, false);
                    self.selecting = true;
                } else {
                    // the first click into the url bar selects the whole url,
                    // so typing replaces it.
                    self.focus_url();
                    self.focus_visible = false;
                }
                None
            }
            Hit::Tab(i) => {
                self.blur();
                self.select_tab(i)
            }
            _ => {
                self.blur();
                None
            }
        }
    }

    pub fn pointer_up(&mut self, p: Point) -> Option<Action> {
        self.selecting = false;
        let pressed = self.pressed.take()?;
        if pressed != self.hit(p) {
            return None;
        }
        self.activate(pressed)
    }

    fn select_tab(&mut self, i: usize) -> Option<Action> {
        if i == self.active_tab || i >= self.tabs.len() {
            return None;
        }
        self.active_tab = i;
        Some(Action::SelectTab(i))
    }

    fn activate(&mut self, hit: Hit) -> Option<Action> {
        if !self.enabled(hit) {
            return None;
        }
        match hit {
            Hit::Back => Some(Action::Back),
            Hit::Forward => Some(Action::Forward),
            Hit::Reload => Some(Action::Reload),
            Hit::Tab(i) => self.select_tab(i),
            _ => None,
        }
    }

    /// enter or space on a focused button or tab. false if focus is
    /// elsewhere.
    pub fn activate_focused(&mut self) -> Option<Option<Action>> {
        match self.focus {
            Some(hit @ (Hit::Back | Hit::Forward | Hit::Reload | Hit::Tab(_))) => {
                Some(self.activate(hit))
            }
            _ => None,
        }
    }

    /// moves keyboard focus through tabs, buttons and the url bar, skipping
    /// disabled buttons.
    pub fn focus_next(&mut self, backward: bool) {
        let order: Vec<Hit> = (0..self.tabs.len())
            .map(Hit::Tab)
            .chain(BUTTONS)
            .chain([Hit::UrlBar])
            .filter(|&h| self.enabled(h))
            .collect();
        let n = order.len();
        let current = self.focus.and_then(|f| order.iter().position(|&h| h == f));
        let next = match (current, backward) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(i), false) => (i + 1) % n,
            (Some(i), true) => (i + n - 1) % n,
        };
        self.selecting = false;
        if order[next] == Hit::UrlBar {
            self.focus_url();
        } else {
            self.focus = Some(order[next]);
        }
        self.focus_visible = true;
    }

    pub fn focus_url(&mut self) {
        self.focus = Some(Hit::UrlBar);
        self.focus_visible = true;
        self.url.select_all();
    }

    pub fn blur(&mut self) {
        self.focus = None;
        self.selecting = false;
    }

    /// enter in the url bar.
    pub fn submit_url(&mut self) -> Option<Action> {
        let text = self.url.text().trim().to_owned();
        self.blur();
        if text.is_empty() {
            self.url.set_text(&self.committed_url);
            return None;
        }
        self.url.set_caret(0, false);
        self.url_scroll = 0.0;
        Some(Action::Navigate(text))
    }

    /// escape drops a url edit and leaves the chrome.
    pub fn cancel(&mut self) {
        self.blur();
        self.url.set_text(&self.committed_url);
        self.url_scroll = 0.0;
    }

    fn snap(&self, v: f32) -> f32 {
        (v * self.scale).round() / self.scale
    }

    /// `r` with every edge on a device pixel boundary.
    fn snap_rect(&self, r: Rect) -> Rect {
        let (x0, y0) = (self.snap(r.min_x()), self.snap(r.min_y()));
        let (x1, y1) = (self.snap(r.max_x()), self.snap(r.max_y()));
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// one device pixel, at least, in css pixels.
    fn hairline(&self) -> f32 {
        self.scale.round().max(1.0) / self.scale
    }

    fn ring(&self, list: &mut DisplayList, rect: Rect, radius: f32) {
        let width = self.snap(FOCUS_RING_WIDTH).max(self.hairline());
        list.push(DisplayItem::Border {
            rect: self.snap_rect(rect),
            radii: CornerRadii::uniform(radius),
            widths: Sides::all(width),
            colors: Sides::all(FOCUS_RING),
        });
    }

    fn ring_visible(&self, hit: Hit) -> bool {
        self.focus_visible && self.focus == Some(hit)
    }

    /// whether `hit` is held down with the pointer still over it.
    fn is_pressed(&self, hit: Hit) -> bool {
        self.pressed == Some(hit) && self.hover == hit
    }

    pub fn paint(&mut self, font: Option<(&ChromeFont, FontId)>, icons: &Icons) -> DisplayList {
        let (width, _) = self.size;
        let mut list = DisplayList::new();

        list.rect(
            self.snap_rect(Rect::new(0.0, 0.0, width, TAB_STRIP_HEIGHT)),
            STRIP_BG,
        );
        for i in 0..self.tabs.len() {
            self.paint_tab(&mut list, i, font);
        }

        list.rect(
            self.snap_rect(Rect::new(0.0, TAB_STRIP_HEIGHT, width, TOOLBAR_HEIGHT)),
            TOOLBAR_BG,
        );
        let line_y = self.snap(TAB_STRIP_HEIGHT + TOOLBAR_HEIGHT);
        list.rect(
            Rect::new(0.0, line_y, self.snap(width), self.hairline()),
            SEPARATOR,
        );

        let button_icons = [&icons.arrow_left, &icons.arrow_right, &icons.rotate_cw];
        for (slot, (hit, icon)) in BUTTONS.into_iter().zip(button_icons).enumerate() {
            self.paint_button(&mut list, slot, hit, icon);
        }
        self.paint_url_bar(&mut list, font);
        list
    }

    fn paint_tab(&self, list: &mut DisplayList, i: usize, font: Option<(&ChromeFont, FontId)>) {
        let rect = self.tab_rect(i);
        if rect.size.width <= 0.0 {
            return;
        }
        let hit = Hit::Tab(i);
        let active = i == self.active_tab;
        let top_radii = CornerRadii {
            top_left: 8.0,
            top_right: 8.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        };
        // inactive tabs draw their hover and pressed backgrounds as a pill
        // inset from the tab's edges.
        let pill = Rect::new(
            rect.min_x() + GAP / 2.0,
            rect.min_y() + GAP / 2.0,
            rect.size.width - GAP,
            rect.size.height - GAP * 1.5,
        );
        if active {
            list.rounded_rect(self.snap_rect(rect), top_radii, TOOLBAR_BG);
        } else if self.is_pressed(hit) {
            list.rounded_rect(
                self.snap_rect(pill),
                CornerRadii::uniform(6.0),
                TAB_PRESSED_BG,
            );
        } else if self.hover == hit {
            list.rounded_rect(
                self.snap_rect(pill),
                CornerRadii::uniform(6.0),
                TAB_HOVER_BG,
            );
        } else if i + 1 != self.active_tab && i + 1 < self.tabs.len() {
            // the separator between two inactive tabs.
            list.rect(
                Rect::new(
                    self.snap(rect.max_x()) - self.hairline(),
                    self.snap(rect.min_y() + 8.0),
                    self.hairline(),
                    self.snap(rect.size.height - 16.0),
                ),
                SEPARATOR,
            );
        }
        if self.ring_visible(hit) {
            self.ring(list, pill, 6.0);
        }
        let Some((font, id)) = font else { return };
        let title = match self.tabs[i].as_str() {
            "" => "New Tab",
            t => t,
        };
        let line = font.shape(title, TAB_FONT_SIZE);
        let text_rect = self.snap_rect(Rect::new(
            rect.min_x() + TAB_PADDING,
            rect.min_y(),
            (rect.size.width - 2.0 * TAB_PADDING).max(0.0),
            rect.size.height,
        ));
        let baseline = self.baseline_in(font, TAB_FONT_SIZE, rect);
        list.push(DisplayItem::PushClip {
            rect: text_rect,
            radii: CornerRadii::default(),
        });
        let color = if active { TEXT } else { MUTED_TEXT };
        list.push(DisplayItem::GlyphRun(line.run(
            id,
            TAB_FONT_SIZE,
            color,
            Point::new(text_rect.min_x(), baseline),
        )));
        list.push(DisplayItem::Pop);
    }

    fn paint_button(&self, list: &mut DisplayList, slot: usize, hit: Hit, icon: &Icon) {
        let rect = self.button_rect(slot);
        let enabled = self.enabled(hit);
        let round = CornerRadii::uniform(BUTTON_SIZE / 2.0);
        if enabled && self.is_pressed(hit) {
            list.rounded_rect(self.snap_rect(rect), round, BUTTON_PRESSED);
        } else if enabled && self.hover == hit {
            list.rounded_rect(self.snap_rect(rect), round, BUTTON_HOVER);
        }
        if self.ring_visible(hit) {
            self.ring(list, rect, BUTTON_SIZE / 2.0);
        }
        let color = if enabled { ICON } else { ICON_DISABLED };
        let inset = (BUTTON_SIZE - ICON_SIZE) / 2.0;
        icon.draw(
            list,
            Point::new(rect.min_x() + inset, rect.min_y() + inset),
            ICON_SIZE,
            self.scale,
            color,
        );
    }

    fn paint_url_bar(&mut self, list: &mut DisplayList, font: Option<(&ChromeFont, FontId)>) {
        let rect = self.snap_rect(self.url_rect());
        if rect.size.width <= 0.0 {
            return;
        }
        let focused = self.url_focused();
        let radii = CornerRadii::uniform(rect.size.height / 2.0);
        if focused {
            list.rounded_rect(rect, radii, TOOLBAR_BG);
            self.ring(list, rect, rect.size.height / 2.0);
        } else {
            let bg = if self.hover == Hit::UrlBar {
                URL_HOVER_BG
            } else {
                URL_BG
            };
            list.rounded_rect(rect, radii, bg);
        }
        let Some((font, id)) = font else {
            self.url_line = None;
            return;
        };
        let line = font.shape(self.url.text(), URL_FONT_SIZE);
        let inner = Rect::new(
            rect.min_x() + URL_PADDING,
            rect.min_y(),
            (rect.size.width - 2.0 * URL_PADDING).max(0.0),
            rect.size.height,
        );
        let caret_x = line.caret_x(self.url.caret());
        if !focused {
            self.url_scroll = 0.0;
        } else {
            // keep the caret inside the field.
            if caret_x - self.url_scroll > inner.size.width {
                self.url_scroll = caret_x - inner.size.width;
            } else if caret_x < self.url_scroll {
                self.url_scroll = caret_x;
            }
            // no empty space after the end of the text.
            self.url_scroll = self.url_scroll.min(line.width - inner.size.width).max(0.0);
        }
        let x0 = self.snap(inner.min_x() - self.url_scroll);
        let baseline = self.baseline_in(font, URL_FONT_SIZE, rect);
        let top = self.snap(baseline - font.ascent(URL_FONT_SIZE));
        let bottom = self.snap(baseline + font.descent(URL_FONT_SIZE));

        list.push(DisplayItem::PushClip {
            rect: self.snap_rect(Rect::new(
                inner.min_x() - 2.0,
                inner.min_y(),
                inner.size.width + 4.0,
                inner.size.height,
            )),
            radii: CornerRadii::default(),
        });
        if focused && self.url.has_selection() {
            let (start, end) = self.url.selection();
            let (sx, ex) = (
                self.snap(x0 + line.caret_x(start)),
                self.snap(x0 + line.caret_x(end)),
            );
            list.rect(Rect::new(sx, top, ex - sx, bottom - top), SELECTION);
        }
        list.push(DisplayItem::GlyphRun(line.run(
            id,
            URL_FONT_SIZE,
            TEXT,
            Point::new(x0, baseline),
        )));
        if focused && !self.url.has_selection() {
            list.rect(
                Rect::new(self.snap(x0 + caret_x), top, self.hairline(), bottom - top),
                TEXT,
            );
        }
        list.push(DisplayItem::Pop);
        self.url_line = Some(line);
    }

    /// the baseline that centers a line of text vertically in `rect`, on a
    /// device pixel.
    fn baseline_in(&self, font: &ChromeFont, size: f32, rect: Rect) -> f32 {
        let (a, d) = (font.ascent(size), font.descent(size));
        self.snap(rect.min_y() + (rect.size.height + a - d) / 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chrome() -> Chrome {
        let mut c = Chrome::new();
        c.set_size(800.0, 600.0, 1.0);
        c.tabs = vec!["one".into(), "two".into()];
        c.set_url("https://example.com/");
        c
    }

    fn center(r: Rect) -> Point {
        Point::new(
            r.min_x() + r.size.width / 2.0,
            r.min_y() + r.size.height / 2.0,
        )
    }

    #[test]
    fn hit_testing_finds_each_control() {
        let c = chrome();
        assert_eq!(c.hit(center(c.tab_rect(1))), Hit::Tab(1));
        assert_eq!(c.hit(center(c.button_rect(0))), Hit::Back);
        assert_eq!(c.hit(center(c.button_rect(1))), Hit::Forward);
        assert_eq!(c.hit(center(c.button_rect(2))), Hit::Reload);
        assert_eq!(c.hit(center(c.url_rect())), Hit::UrlBar);
        assert_eq!(c.hit(Point::new(10.0, 300.0)), Hit::Content);
        assert_eq!(
            c.content_rect(),
            Rect::new(0.0, CHROME_HEIGHT, 800.0, 600.0 - CHROME_HEIGHT)
        );
    }

    #[test]
    fn buttons_fire_on_release_over_the_same_button() {
        let mut c = chrome();
        c.can_go_back = true;
        let back = center(c.button_rect(0));
        assert_eq!(c.pointer_down(back), None);
        assert_eq!(c.pointer_up(back), Some(Action::Back));
        c.pointer_down(back);
        assert_eq!(c.pointer_up(center(c.button_rect(2))), None);
        let reload = center(c.button_rect(2));
        c.pointer_down(reload);
        assert_eq!(c.pointer_up(reload), Some(Action::Reload));
    }

    #[test]
    fn disabled_buttons_do_nothing() {
        let mut c = chrome();
        let fwd = center(c.button_rect(1));
        c.pointer_down(fwd);
        assert_eq!(c.pointer_up(fwd), None);
    }

    #[test]
    fn clicking_a_tab_selects_it() {
        let mut c = chrome();
        assert_eq!(
            c.pointer_down(center(c.tab_rect(1))),
            Some(Action::SelectTab(1))
        );
        assert_eq!(c.active_tab, 1);
        assert_eq!(c.pointer_down(center(c.tab_rect(1))), None);
    }

    #[test]
    fn url_bar_focus_edit_and_submit() {
        let mut c = chrome();
        c.pointer_down(center(c.url_rect()));
        assert!(c.url_focused());
        assert_eq!(c.url_field().selection(), (0, "https://example.com/".len()));
        c.url_field().insert("example.org");
        assert_eq!(c.submit_url(), Some(Action::Navigate("example.org".into())));
        assert!(!c.url_focused());
    }

    #[test]
    fn escape_restores_the_committed_url() {
        let mut c = chrome();
        c.focus_url();
        c.url_field().insert("typo");
        c.cancel();
        assert_eq!(c.url_field().text(), "https://example.com/");
    }

    #[test]
    fn set_url_while_editing_keeps_the_edit() {
        let mut c = chrome();
        c.focus_url();
        c.url_field().insert("half typed");
        c.set_url("https://example.net/");
        assert_eq!(c.url_field().text(), "half typed");
        c.cancel();
        assert_eq!(c.url_field().text(), "https://example.net/");
    }

    #[test]
    fn empty_submit_restores_and_does_not_navigate() {
        let mut c = chrome();
        c.focus_url();
        c.url_field().insert("   ");
        assert_eq!(c.submit_url(), None);
        assert_eq!(c.url_field().text(), "https://example.com/");
    }

    #[test]
    fn keyboard_focus_cycles_and_skips_disabled_buttons() {
        let mut c = chrome();
        c.can_go_forward = true;
        let mut seen = Vec::new();
        for _ in 0..6 {
            c.focus_next(false);
            seen.push(c.focus);
        }
        assert_eq!(
            seen,
            [
                Some(Hit::Tab(0)),
                Some(Hit::Tab(1)),
                Some(Hit::Forward),
                Some(Hit::Reload),
                Some(Hit::UrlBar),
                Some(Hit::Tab(0)),
            ]
        );
        c.focus_next(true);
        assert_eq!(c.focus, Some(Hit::UrlBar));
        assert_eq!(c.url_field().selection(), (0, "https://example.com/".len()));
    }

    #[test]
    fn enter_activates_the_focused_control() {
        let mut c = chrome();
        c.focus_next(false);
        c.focus_next(false);
        assert_eq!(c.activate_focused(), Some(Some(Action::SelectTab(1))));
        c.focus_next(false);
        assert_eq!(c.focus, Some(Hit::Reload));
        assert_eq!(c.activate_focused(), Some(Some(Action::Reload)));
        c.focus_url();
        assert_eq!(c.activate_focused(), None);
    }

    fn rects(list: &DisplayList) -> Vec<Rect> {
        list.items
            .iter()
            .filter_map(|i| match i {
                DisplayItem::Rect { rect, .. }
                | DisplayItem::RoundedRect { rect, .. }
                | DisplayItem::Border { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn edges_land_on_device_pixels() {
        let icons = Icons::load();
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let mut c = chrome();
            c.set_size(801.0, 600.0, scale);
            c.can_go_back = true;
            c.hover = Hit::Back;
            c.focus_url();
            for r in rects(&c.paint(None, &icons)) {
                for v in [r.min_x(), r.min_y(), r.max_x(), r.max_y()] {
                    let device = v * scale;
                    assert!((device - device.round()).abs() < 1e-3, "{v} at {scale}");
                }
            }
        }
    }

    #[test]
    fn paint_is_balanced_and_works_without_a_font() {
        let mut c = chrome();
        c.focus_url();
        let list = c.paint(None, &Icons::load());
        let mut depth = 0i32;
        for item in &list.items {
            match item {
                DisplayItem::PushClip { .. }
                | DisplayItem::PushTransform(_)
                | DisplayItem::PushOpacity(_)
                | DisplayItem::PushScrollFrame { .. } => depth += 1,
                DisplayItem::Pop => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0);
        }
        assert_eq!(depth, 0);
        assert!(!list.is_empty());
    }
}
