//! the browser window and its chrome (docs/design/ui.md).
//!
//! [`Ui`] owns the winit event loop and one window titled "Super Browser".
//! the chrome (tab strip, back, forward, reload, url bar) is drawn by the
//! compositor from a display list this crate builds, above the content
//! display list the browser hands in through [`UiProxy`]. user actions reach
//! the browser through [`Callbacks`].

mod chrome;
mod field;
mod icons;
mod text;

use std::fmt;
use std::sync::Arc;

use sp_compositor::{Compositor, WindowSurface};
use sp_paint::{Color, DisplayList, FontId, GlyphRun, ImageId, Point, Size};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::chrome::{Action, Chrome, Hit};
use crate::icons::Icons;
use crate::text::ChromeFont;

pub use sp_compositor::Blob;

/// the font id the chrome's own font is registered under. content display
/// lists must not use it.
pub const CHROME_FONT_ID: FontId = FontId(u64::MAX);

pub const WINDOW_TITLE: &str = "Super Browser";

/// what the user asked the browser to do. every method has a no-op default.
pub trait Callbacks {
    /// enter in the url bar, with the text as typed (trimmed). url fixup is
    /// the browser's job.
    fn navigate(&mut self, _input: &str) {}
    fn back(&mut self) {}
    fn forward(&mut self) {}
    fn reload(&mut self) {}
    fn select_tab(&mut self, _index: usize) {}
    /// the content area changed size, in css pixels.
    fn viewport_resized(&mut self, _size: Size) {}
}

#[derive(Debug)]
pub enum UiError {
    EventLoop(String),
    Window(String),
    Compositor(sp_compositor::Error),
}

impl fmt::Display for UiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UiError::EventLoop(e) => write!(f, "event loop: {e}"),
            UiError::Window(e) => write!(f, "window: {e}"),
            UiError::Compositor(e) => write!(f, "compositor: {e}"),
        }
    }
}

impl std::error::Error for UiError {}

enum Command {
    SetContent(DisplayList),
    SetTabs(Vec<String>, usize),
    SetTabTitle(usize, String),
    SetUrl(String),
    SetNavigationState { back: bool, forward: bool },
    AddFont(FontId, Blob<u8>, u32),
    AddImage(ImageId, u32, u32, Vec<u8>),
    Close,
}

/// sends state to the ui from any thread. calls after the window closed are
/// dropped.
#[derive(Clone)]
pub struct UiProxy(EventLoopProxy<Command>);

impl UiProxy {
    fn send(&self, command: Command) {
        // the only error is a closed event loop, after which nothing is shown.
        let _ = self.0.send_event(command);
    }

    /// the page's display list, in css pixels relative to the content area.
    pub fn set_content(&self, list: DisplayList) {
        self.send(Command::SetContent(list));
    }

    pub fn set_tabs(&self, titles: Vec<String>, active: usize) {
        self.send(Command::SetTabs(titles, active));
    }

    pub fn set_tab_title(&self, index: usize, title: impl Into<String>) {
        self.send(Command::SetTabTitle(index, title.into()));
    }

    /// the active page's url, shown in the url bar when it isn't being
    /// edited.
    pub fn set_url(&self, url: impl Into<String>) {
        self.send(Command::SetUrl(url.into()));
    }

    pub fn set_navigation_state(&self, can_go_back: bool, can_go_forward: bool) {
        self.send(Command::SetNavigationState {
            back: can_go_back,
            forward: can_go_forward,
        });
    }

    /// font bytes for glyph runs that use `id`. `index` picks a face in a
    /// collection.
    pub fn add_font(&self, id: FontId, bytes: Blob<u8>, index: u32) {
        self.send(Command::AddFont(id, bytes, index));
    }

    /// non-premultiplied rgba8 pixels for image items that use `id`.
    pub fn add_image(&self, id: ImageId, width: u32, height: u32, rgba: Vec<u8>) {
        self.send(Command::AddImage(id, width, height, rgba));
    }

    pub fn close(&self) {
        self.send(Command::Close);
    }
}

pub struct Ui {
    event_loop: EventLoop<Command>,
    font: Option<ChromeFont>,
}

impl Ui {
    pub fn new() -> Result<Self, UiError> {
        let event_loop = EventLoop::with_user_event()
            .build()
            .map_err(|e| UiError::EventLoop(e.to_string()))?;
        let font = ChromeFont::load_system();
        if font.is_none() {
            eprintln!("sp_ui: no system ui font found, chrome text will not be drawn");
        }
        Ok(Self { event_loop, font })
    }

    pub fn proxy(&self) -> UiProxy {
        UiProxy(self.event_loop.create_proxy())
    }

    /// shapes `text` with the chrome font into a glyph run (font
    /// [`CHROME_FONT_ID`]) with its baseline starting at `origin`. none if no
    /// system font was found.
    pub fn shape_text(
        &self,
        text: &str,
        size: f32,
        color: Color,
        origin: Point,
    ) -> Option<GlyphRun> {
        let font = self.font.as_ref()?;
        Some(
            font.shape(text, size)
                .run(CHROME_FONT_ID, size, color, origin),
        )
    }

    /// runs the event loop until the window closes.
    pub fn run(self, callbacks: impl Callbacks) -> Result<(), UiError> {
        let mut app = App {
            callbacks,
            font: self.font,
            icons: Icons::load(),
            chrome: Chrome::new(),
            window: None,
            pending: Vec::new(),
            content: DisplayList::new(),
            cursor: Point::default(),
            modifiers: ModifiersState::empty(),
            error: None,
        };
        self.event_loop
            .run_app(&mut app)
            .map_err(|e| UiError::EventLoop(e.to_string()))?;
        match app.error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

struct WindowState {
    window: Arc<Window>,
    compositor: Compositor,
    surface: WindowSurface,
}

struct App<C> {
    callbacks: C,
    font: Option<ChromeFont>,
    icons: Icons,
    chrome: Chrome,
    window: Option<WindowState>,
    /// resources that arrived before the compositor existed.
    pending: Vec<Command>,
    content: DisplayList,
    /// pointer position in css pixels.
    cursor: Point,
    modifiers: ModifiersState,
    error: Option<UiError>,
}

impl<C: Callbacks> App<C> {
    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), UiError> {
        let attrs = Window::default_attributes()
            .with_title(WINDOW_TITLE)
            .with_inner_size(LogicalSize::new(1200.0, 800.0))
            .with_min_inner_size(LogicalSize::new(400.0, 200.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| UiError::Window(e.to_string()))?,
        );
        let size = window.inner_size();
        let (mut compositor, surface) =
            Compositor::for_window(window.clone(), size.width, size.height)
                .map_err(UiError::Compositor)?;
        compositor.set_scale(window.scale_factor());
        if let Some(font) = &self.font
            && let Err(e) =
                compositor
                    .resources_mut()
                    .add_font(CHROME_FONT_ID, font.blob.clone(), font.index)
        {
            eprintln!("sp_ui: chrome font rejected: {e}");
        }
        self.window = Some(WindowState {
            window,
            compositor,
            surface,
        });
        for command in std::mem::take(&mut self.pending) {
            self.apply(command, event_loop);
        }
        self.resized();
        Ok(())
    }

    fn apply(&mut self, command: Command, event_loop: &ActiveEventLoop) {
        match command {
            Command::SetContent(list) => {
                self.content = list;
                if let Some(w) = &mut self.window {
                    w.compositor
                        .set_content(self.content.clone(), self.chrome.content_rect());
                }
            }
            Command::SetTabs(titles, active) => {
                self.chrome.tabs = if titles.is_empty() {
                    vec![String::new()]
                } else {
                    titles
                };
                self.chrome.active_tab = active.min(self.chrome.tabs.len() - 1);
            }
            Command::SetTabTitle(i, title) => {
                if let Some(t) = self.chrome.tabs.get_mut(i) {
                    *t = title;
                }
            }
            Command::SetUrl(url) => self.chrome.set_url(&url),
            Command::SetNavigationState { back, forward } => {
                self.chrome.can_go_back = back;
                self.chrome.can_go_forward = forward;
            }
            Command::AddFont(id, bytes, index) => match &mut self.window {
                Some(w) => {
                    if let Err(e) = w.compositor.resources_mut().add_font(id, bytes, index) {
                        eprintln!("sp_ui: font {id:?} rejected: {e}");
                    }
                }
                None => self.pending.push(Command::AddFont(id, bytes, index)),
            },
            Command::AddImage(id, width, height, rgba) => match &mut self.window {
                Some(w) => {
                    if let Err(e) = w
                        .compositor
                        .resources_mut()
                        .add_image(id, width, height, rgba)
                    {
                        eprintln!("sp_ui: image {id:?} rejected: {e}");
                    }
                }
                None => self
                    .pending
                    .push(Command::AddImage(id, width, height, rgba)),
            },
            Command::Close => event_loop.exit(),
        }
        self.redraw();
    }

    /// repaints the chrome and asks for a frame if anything changed.
    fn redraw(&mut self) {
        let Some(w) = &mut self.window else { return };
        let font = self.font.as_ref().map(|f| (f, CHROME_FONT_ID));
        w.compositor
            .set_chrome(self.chrome.paint(font, &self.icons));
        if w.compositor.needs_render() {
            w.window.request_redraw();
        }
    }

    fn resized(&mut self) {
        let Some(w) = &mut self.window else { return };
        let size = w.window.inner_size();
        let scale = w.window.scale_factor();
        w.compositor.resize(&mut w.surface, size.width, size.height);
        w.compositor.set_scale(scale);
        self.chrome.set_size(
            (f64::from(size.width) / scale) as f32,
            (f64::from(size.height) / scale) as f32,
            scale as f32,
        );
        let content = self.chrome.content_rect();
        w.compositor.set_content_rect(content);
        self.callbacks.viewport_resized(content.size);
        self.redraw();
    }

    fn act(&mut self, action: Option<Action>) {
        match action {
            Some(Action::Navigate(input)) => self.callbacks.navigate(&input),
            Some(Action::Back) => self.callbacks.back(),
            Some(Action::Forward) => self.callbacks.forward(),
            Some(Action::Reload) => self.callbacks.reload(),
            Some(Action::SelectTab(i)) => self.callbacks.select_tab(i),
            None => {}
        }
    }

    fn update_ime(&self) {
        if let Some(w) = &self.window {
            w.window.set_ime_allowed(self.chrome.url_focused());
        }
    }

    fn key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let shortcut = self.modifiers.control_key() || self.modifiers.super_key();
        let shift = self.modifiers.shift_key();
        let alt = self.modifiers.alt_key();
        // browser-wide shortcuts.
        match &event.logical_key {
            Key::Character(c) if shortcut && c.eq_ignore_ascii_case("l") => {
                self.chrome.focus_url();
                return;
            }
            Key::Character(c) if shortcut && c.eq_ignore_ascii_case("r") => {
                self.callbacks.reload();
                return;
            }
            Key::Named(NamedKey::F5) => {
                self.callbacks.reload();
                return;
            }
            Key::Named(NamedKey::ArrowLeft) if alt && self.chrome.can_go_back => {
                self.callbacks.back();
                return;
            }
            Key::Named(NamedKey::ArrowRight) if alt && self.chrome.can_go_forward => {
                self.callbacks.forward();
                return;
            }
            _ => {}
        }
        match &event.logical_key {
            Key::Named(NamedKey::Tab) if !shortcut => {
                self.chrome.focus_next(shift);
                return;
            }
            Key::Named(NamedKey::Escape) => {
                self.chrome.cancel();
                return;
            }
            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                if let Some(action) = self.chrome.activate_focused() {
                    self.act(action);
                    return;
                }
            }
            _ => {}
        }
        if !self.chrome.url_focused() {
            return;
        }
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => {
                let action = self.chrome.submit_url();
                self.act(action);
            }
            Key::Named(NamedKey::Backspace) => self.chrome.url_field().backspace(),
            Key::Named(NamedKey::Delete) => self.chrome.url_field().delete(),
            Key::Named(NamedKey::ArrowLeft) => self.chrome.url_field().left(shift),
            Key::Named(NamedKey::ArrowRight) => self.chrome.url_field().right(shift),
            Key::Named(NamedKey::Home) => self.chrome.url_field().home(shift),
            Key::Named(NamedKey::End) => self.chrome.url_field().end(shift),
            Key::Character(c) if shortcut && c.eq_ignore_ascii_case("a") => {
                self.chrome.url_field().select_all();
            }
            _ if !shortcut => {
                if let Some(text) = &event.text {
                    self.chrome.url_field().insert(text);
                }
            }
            _ => {}
        }
    }
}

impl<C: Callbacks> ApplicationHandler<Command> for App<C> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(e) = self.create_window(event_loop) {
            self.error = Some(e);
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, command: Command) {
        self.apply(command, event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resized(),
            WindowEvent::RedrawRequested => {
                let Some(w) = &mut self.window else { return };
                // a skipped frame (occluded, busy or outdated surface) stays
                // dirty and is drawn on the next resize, unocclusion or change.
                match w.compositor.present(&mut w.surface) {
                    Ok(_) => {}
                    Err(e) => {
                        self.error = Some(UiError::Compositor(e));
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::Occluded(false) => {
                if let Some(w) = &mut self.window {
                    w.compositor.invalidate();
                    w.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.modifiers = m.state(),
            WindowEvent::CursorMoved { position, .. } => {
                let Some(w) = &self.window else { return };
                let PhysicalPosition { x, y } = position;
                let scale = w.window.scale_factor();
                self.cursor = Point::new((x / scale) as f32, (y / scale) as f32);
                let icon = match self.chrome.hit(self.cursor) {
                    Hit::UrlBar => CursorIcon::Text,
                    _ => CursorIcon::Default,
                };
                w.window.set_cursor(icon);
                if self.chrome.pointer_move(self.cursor) {
                    self.redraw();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.chrome.pointer_left() {
                    self.redraw();
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                let action = match state {
                    ElementState::Pressed => self.chrome.pointer_down(self.cursor),
                    ElementState::Released => self.chrome.pointer_up(self.cursor),
                };
                self.act(action);
                self.update_ime();
                self.redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.key(&event);
                self.update_ime();
                self.redraw();
            }
            WindowEvent::Ime(Ime::Commit(text)) if self.chrome.url_focused() => {
                self.chrome.url_field().insert(&text);
                self.redraw();
            }
            _ => {}
        }
    }
}
