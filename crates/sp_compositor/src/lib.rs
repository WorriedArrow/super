//! the compositor (docs/design/engine.md, "compositor").
//!
//! it holds the current content display list and the browser chrome's
//! display list, builds one vello scene from them and renders it either to a
//! window surface or to an offscreen texture that is read back as rgba8.
//! window rendering happens only when something changed since the last
//! present.

mod gpu;
mod scene;

use std::collections::HashMap;
use std::fmt;

use sp_paint::{Color, DisplayList, FontId, ImageId, Rect, ScrollFrameId};
use vello::kurbo::Affine;
use vello::peniko::{FontData, ImageAlphaType, ImageData, ImageFormat};
use vello::{AaConfig, AaSupport, RenderParams, Renderer, RendererOptions, Scene};

pub use gpu::WindowSurface;
pub use vello::kurbo;
pub use vello::peniko::Blob;
pub use vello::wgpu::SurfaceTarget;

use crate::gpu::Gpu;
use crate::scene::SceneBuilder;

/// the largest image side the compositor accepts. it is wgpu's default
/// max_texture_dimension_2d, which bounds vello's image atlas.
pub const MAX_IMAGE_SIDE: u32 = 8192;

#[derive(Debug)]
pub enum Error {
    /// no gpu adapter is available.
    NoAdapter(String),
    Device(String),
    Surface(String),
    Renderer(String),
    Readback(String),
    /// image pixels don't match the stated size, or the size is out of range.
    InvalidImage,
    /// a font index doesn't name a font in the given bytes.
    InvalidFont,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoAdapter(e) => write!(f, "no gpu adapter: {e}"),
            Error::Device(e) => write!(f, "gpu device: {e}"),
            Error::Surface(e) => write!(f, "window surface: {e}"),
            Error::Renderer(e) => write!(f, "vello: {e}"),
            Error::Readback(e) => write!(f, "readback: {e}"),
            Error::InvalidImage => f.write_str("image size and pixel data disagree"),
            Error::InvalidFont => f.write_str("font bytes hold no font at that index"),
        }
    }
}

impl std::error::Error for Error {}

/// font and image data referenced by display lists, by id. each arrives
/// once and stays until removed.
#[derive(Default)]
pub struct Resources {
    fonts: HashMap<FontId, FontData>,
    images: HashMap<ImageId, ImageData>,
}

impl Resources {
    /// `index` selects a face in a font collection, 0 otherwise.
    pub fn add_font(&mut self, id: FontId, bytes: Blob<u8>, index: u32) -> Result<(), Error> {
        use skrifa::raw::TableProvider;
        // vello unwraps both of these while drawing glyphs.
        let valid =
            skrifa::FontRef::from_index(bytes.data(), index).is_ok_and(|font| font.head().is_ok());
        if !valid {
            return Err(Error::InvalidFont);
        }
        self.fonts.insert(id, FontData::new(bytes, index));
        Ok(())
    }

    pub fn remove_font(&mut self, id: FontId) {
        self.fonts.remove(&id);
    }

    /// `rgba` is non-premultiplied rgba8, row-major, `width * height * 4`
    /// bytes.
    pub fn add_image(
        &mut self,
        id: ImageId,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), Error> {
        let in_range =
            (1..=MAX_IMAGE_SIDE).contains(&width) && (1..=MAX_IMAGE_SIDE).contains(&height);
        if !in_range || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
            return Err(Error::InvalidImage);
        }
        self.images.insert(
            id,
            ImageData {
                data: Blob::from(rgba),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width,
                height,
            },
        );
        Ok(())
    }

    pub fn remove_image(&mut self, id: ImageId) {
        self.images.remove(&id);
    }
}

pub struct Compositor {
    gpu: Gpu,
    renderer: Renderer,
    scene: Scene,
    resources: Resources,
    scroll_offsets: HashMap<ScrollFrameId, (f32, f32)>,
    content: DisplayList,
    content_rect: Rect,
    chrome: DisplayList,
    background: Color,
    scale: f64,
    dirty: bool,
}

impl Compositor {
    /// a compositor with no window, for offscreen rendering.
    pub fn headless() -> Result<Self, Error> {
        Self::new(Gpu::new(None)?)
    }

    /// a compositor that presents to `window`, whose drawable area is
    /// `width` by `height` physical pixels.
    pub fn for_window(
        window: impl Into<SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<(Self, WindowSurface), Error> {
        let (gpu, surface) = Gpu::with_window(window, width, height)?;
        Ok((Self::new(gpu)?, surface))
    }

    fn new(gpu: Gpu) -> Result<Self, Error> {
        let renderer = Renderer::new(
            &gpu.device,
            RendererOptions {
                antialiasing_support: AaSupport::area_only(),
                ..RendererOptions::default()
            },
        )
        .map_err(|e| Error::Renderer(e.to_string()))?;
        Ok(Self {
            gpu,
            renderer,
            scene: Scene::new(),
            resources: Resources::default(),
            scroll_offsets: HashMap::new(),
            content: DisplayList::new(),
            content_rect: Rect::default(),
            chrome: DisplayList::new(),
            background: Color::WHITE,
            scale: 1.0,
            dirty: true,
        })
    }

    pub fn adapter_name(&self) -> String {
        self.gpu.adapter.get_info().name
    }

    pub fn resources(&self) -> &Resources {
        &self.resources
    }

    /// changing resources marks the frame dirty, since lists already set may
    /// reference the ids involved.
    pub fn resources_mut(&mut self) -> &mut Resources {
        self.dirty = true;
        &mut self.resources
    }

    /// the page's display list, drawn translated to `rect`'s origin and
    /// clipped to `rect`, in css pixels.
    pub fn set_content(&mut self, list: DisplayList, rect: Rect) {
        self.content = list;
        self.content_rect = rect;
        self.dirty = true;
    }

    pub fn set_content_rect(&mut self, rect: Rect) {
        if rect != self.content_rect {
            self.content_rect = rect;
            self.dirty = true;
        }
    }

    /// the browser chrome's display list, drawn above the content in window
    /// coordinates (css pixels).
    pub fn set_chrome(&mut self, list: DisplayList) {
        if list != self.chrome {
            self.chrome = list;
            self.dirty = true;
        }
    }

    pub fn set_background(&mut self, color: Color) {
        if color != self.background {
            self.background = color;
            self.dirty = true;
        }
    }

    /// device pixels per css pixel.
    pub fn set_scale(&mut self, scale: f64) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        if scale != self.scale {
            self.scale = scale;
            self.dirty = true;
        }
    }

    pub fn set_scroll_offset(&mut self, id: ScrollFrameId, x: f32, y: f32) {
        if self.scroll_offsets.insert(id, (x, y)) != Some((x, y)) {
            self.dirty = true;
        }
    }

    /// whether something changed since the last present.
    pub fn needs_render(&self) -> bool {
        self.dirty
    }

    /// forces the next [`Compositor::present`] to render, for when the
    /// window system lost the surface contents.
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    pub fn resize(&mut self, surface: &mut WindowSurface, width: u32, height: u32) {
        surface.resize(&self.gpu.device, width, height);
        self.dirty = true;
    }

    /// renders and presents if anything changed since the last present.
    /// returns whether a frame was presented.
    pub fn present(&mut self, surface: &mut WindowSurface) -> Result<bool, Error> {
        if !self.dirty || !surface.is_drawable() {
            return Ok(false);
        }
        let (width, height) = surface.size();
        self.build_scene(width, height);
        let params = self.render_params(width, height);
        let presented = surface.present(&self.gpu, &mut self.renderer, &self.scene, &params)?;
        if presented {
            self.dirty = false;
        }
        Ok(presented)
    }

    /// renders the current frame at `width` by `height` device pixels into
    /// an offscreen texture and returns its pixels: rgba8, row-major, with no
    /// row padding. this path ignores and keeps the dirty state.
    pub fn render_to_rgba(&mut self, width: u32, height: u32) -> Result<Vec<u8>, Error> {
        let max = self.gpu.device.limits().max_texture_dimension_2d;
        if !(1..=max).contains(&width) || !(1..=max).contains(&height) {
            return Err(Error::Readback(format!(
                "{width}x{height} is outside 1..={max} pixels per side"
            )));
        }
        self.build_scene(width, height);
        let params = self.render_params(width, height);
        gpu::render_offscreen(&self.gpu, &mut self.renderer, &self.scene, &params)
    }

    fn render_params(&self, width: u32, height: u32) -> RenderParams {
        RenderParams {
            base_color: scene::to_color(self.background),
            width,
            height,
            antialiasing_method: AaConfig::Area,
        }
    }

    fn build_scene(&mut self, width: u32, height: u32) {
        self.scene.reset();
        let mut builder = SceneBuilder {
            scene: &mut self.scene,
            resources: &self.resources,
            scroll_offsets: &self.scroll_offsets,
            viewport: kurbo::Rect::new(0.0, 0.0, f64::from(width), f64::from(height)),
        };
        let scale = Affine::scale(self.scale);
        let origin = self.content_rect.origin;
        let content_base = scale * Affine::translate((f64::from(origin.x), f64::from(origin.y)));
        let content_clip = Rect {
            origin: sp_paint::Point::default(),
            size: self.content_rect.size,
        };
        builder.append(&self.content, content_base, Some(content_clip));
        builder.append(&self.chrome, scale, None);
    }
}
