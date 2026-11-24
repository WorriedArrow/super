//! wgpu device, window surface and offscreen readback.

use std::future::Future;
use std::sync::{Arc, mpsc};
use std::task::{Context, Poll, Waker};

use vello::wgpu::{self, util::TextureBlitter};
use vello::{RenderParams, Renderer, Scene};

use crate::Error;

pub(crate) struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// wgpu's native futures resolve without a reactor, so polling in a loop is
/// enough. the gpu work they wait on is driven by `device.poll`, not by this.
fn block_on<F: Future>(fut: F) -> F::Output {
    let mut cx = Context::from_waker(Waker::noop());
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
}

impl Gpu {
    pub fn new(surface: Option<(&wgpu::Instance, &wgpu::Surface<'_>)>) -> Result<Self, Error> {
        let owned;
        let (instance, surface) = match surface {
            Some((instance, surface)) => (instance, Some(surface)),
            None => {
                owned = instance();
                (&owned, None)
            }
        };
        let adapter = block_on(wgpu::util::initialize_adapter_from_env_or_default(
            instance, surface,
        ))
        .map_err(|e| Error::NoAdapter(e.to_string()))?;
        // the same optional features vello's own helpers ask for.
        let wanted = wgpu::Features::CLEAR_TEXTURE | wgpu::Features::PIPELINE_CACHE;
        let (device, queue) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("sp_compositor"),
            required_features: adapter.features() & wanted,
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| Error::Device(e.to_string()))?;
        // wgpu panics on uncaptured errors by default. display lists can't
        // cause them (images are size-checked on arrival), so one means a bug
        // in this crate or the driver; keep the browser alive and say so.
        device.on_uncaptured_error(Arc::new(|e| eprintln!("sp_compositor: wgpu error: {e}")));
        Ok(Self {
            adapter,
            device,
            queue,
        })
    }

    pub fn with_window(
        window: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
    ) -> Result<(Self, WindowSurface), Error> {
        let instance = instance();
        let surface = instance
            .create_surface(window)
            .map_err(|e| Error::Surface(e.to_string()))?;
        let gpu = Gpu::new(Some((&instance, &surface)))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        // vello writes srgb-encoded values into an rgba8unorm texture, so the
        // surface must not encode again.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
                )
            })
            .ok_or_else(|| {
                Error::Surface(format!("no non-srgb 8-bit format in {:?}", caps.formats))
            })?;
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        let blitter = TextureBlitter::new(&gpu.device, format);
        let mut window_surface = WindowSurface {
            surface,
            config,
            target: None,
            blitter,
        };
        window_surface.resize(&gpu.device, width, height);
        Ok((gpu, window_surface))
    }
}

/// vello renders with compute shaders into a storage texture, which most
/// surfaces can't be, so frames go to an intermediate texture and are blitted.
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    target: Option<wgpu::TextureView>,
    blitter: TextureBlitter,
}

impl WindowSurface {
    pub(crate) fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    pub(crate) fn is_drawable(&self) -> bool {
        self.target.is_some()
    }

    pub(crate) fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        self.config.width = width;
        self.config.height = height;
        // a minimized window has zero size and can't be configured.
        if width == 0 || height == 0 {
            self.target = None;
            return;
        }
        self.surface.configure(device, &self.config);
        let texture = storage_texture(device, width, height, wgpu::TextureUsages::TEXTURE_BINDING);
        self.target = Some(texture.create_view(&wgpu::TextureViewDescriptor::default()));
    }

    /// returns false when the frame was skipped (surface busy, occluded or
    /// just reconfigured); the caller retries on its next redraw.
    pub(crate) fn present(
        &mut self,
        gpu: &Gpu,
        renderer: &mut Renderer,
        scene: &Scene,
        params: &RenderParams,
    ) -> Result<bool, Error> {
        let Some(target) = &self.target else {
            return Ok(false);
        };
        let (frame, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame) => (frame, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(frame) => (frame, true),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&gpu.device, &self.config);
                return Ok(false);
            }
            other => return Err(Error::Surface(format!("{other:?}"))),
        };
        renderer
            .render_to_texture(&gpu.device, &gpu.queue, scene, target, params)
            .map_err(|e| Error::Renderer(e.to_string()))?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sp_compositor blit"),
            });
        self.blitter.copy(&gpu.device, &mut encoder, target, &view);
        gpu.queue.submit([encoder.finish()]);
        gpu.queue.present(frame);
        if suboptimal {
            self.surface.configure(&gpu.device, &self.config);
        }
        Ok(true)
    }
}

fn storage_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    extra: wgpu::TextureUsages,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sp_compositor target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        usage: wgpu::TextureUsages::STORAGE_BINDING | extra,
        format: wgpu::TextureFormat::Rgba8Unorm,
        view_formats: &[],
    })
}

pub(crate) fn render_offscreen(
    gpu: &Gpu,
    renderer: &mut Renderer,
    scene: &Scene,
    params: &RenderParams,
) -> Result<Vec<u8>, Error> {
    let (width, height) = (params.width, params.height);
    let texture = storage_texture(&gpu.device, width, height, wgpu::TextureUsages::COPY_SRC);
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    renderer
        .render_to_texture(&gpu.device, &gpu.queue, scene, &view, params)
        .map_err(|e| Error::Renderer(e.to_string()))?;

    let row = width as usize * 4;
    let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("sp_compositor readback"),
        size: (padded_row * height as usize) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("sp_compositor readback"),
        });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row as u32),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);

    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|e| Error::Readback(e.to_string()))?;
    rx.recv()
        .map_err(|e| Error::Readback(e.to_string()))?
        .map_err(|e| Error::Readback(e.to_string()))?;
    let mapped = slice
        .get_mapped_range()
        .map_err(|e| Error::Readback(e.to_string()))?;
    let mut pixels = Vec::with_capacity(row * height as usize);
    for chunk in mapped.chunks(padded_row) {
        pixels.extend_from_slice(&chunk[..row]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
