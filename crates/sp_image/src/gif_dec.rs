//! gif decoding and frame compositing per
//! https://www.w3.org/Graphics/GIF/spec-gif89a.txt

use std::num::NonZeroU64;
use std::time::Duration;

use gif::{ColorOutput, DecodeOptions, DisposalMethod, MemoryLimit, Repeat};

use crate::{DecodeError, Frame, Image, Limits, Plays, malformed};

pub(crate) fn decode(bytes: &[u8], limits: Limits) -> Result<Image, DecodeError> {
    let mut options = DecodeOptions::new();
    options.set_color_output(ColorOutput::RGBA);
    // the gif crate limits each frame's buffer on its own; the canvas and the
    // frame count are checked against `limits` below.
    options.set_memory_limit(MemoryLimit::Bytes(
        NonZeroU64::new(limits.bytes(4)).unwrap_or(NonZeroU64::MIN),
    ));
    let mut decoder = options.read_info(bytes).map_err(malformed)?;
    // section 18: the logical screen descriptor gives the canvas size.
    let screen = (u32::from(decoder.width()), u32::from(decoder.height()));

    let mut canvas: Option<Canvas> = None;
    let mut frames = Vec::new();
    loop {
        let frame = match decoder.read_next_frame() {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(err) if frames.is_empty() => return Err(malformed(err)),
            // a corrupt or truncated frame ends the animation at the last
            // complete frame.
            Err(_) => break,
        };
        let rect = Rect {
            left: u32::from(frame.left),
            top: u32::from(frame.top),
            width: u32::from(frame.width),
            height: u32::from(frame.height),
        };
        let canvas = match &mut canvas {
            Some(canvas) => canvas,
            None => {
                // a zero-sized logical screen can't show anything; size the
                // canvas to the first frame instead.
                let (width, height) = if screen.0 == 0 || screen.1 == 0 {
                    (rect.left + rect.width, rect.top + rect.height)
                } else {
                    screen
                };
                limits.check(width, height, 1)?;
                canvas.insert(Canvas::new(width, height))
            }
        };
        limits.check(canvas.width, canvas.height, frames.len() as u64 + 1)?;

        // section 23: "restore to previous" puts back the canvas as it was
        // before this frame was drawn.
        let previous = (frame.dispose == DisposalMethod::Previous).then(|| canvas.pixels.clone());
        canvas.draw(&rect, &frame.buffer);
        frames.push(Frame {
            pixels: canvas.pixels.clone(),
            // section 23: the delay time is in hundredths of a second.
            delay: Duration::from_millis(u64::from(frame.delay) * 10),
        });
        // section 23: disposal applies after the frame has been displayed.
        match frame.dispose {
            DisposalMethod::Any | DisposalMethod::Keep => {}
            // the background is transparent, as in every current browser.
            DisposalMethod::Background => canvas.clear(&rect),
            DisposalMethod::Previous => {
                if let Some(previous) = previous {
                    canvas.pixels = previous;
                }
            }
        }
    }

    let Some(canvas) = canvas else {
        return Err(malformed("gif has no frames"));
    };
    let plays = if frames.len() == 1 {
        Plays::Times(1)
    } else {
        match decoder.repeat() {
            Repeat::Infinite => Plays::Infinite,
            // the NETSCAPE2.0 loop count is how many times the animation
            // repeats after the first play. without the extension it plays
            // once.
            Repeat::Finite(n) => Plays::Times(u32::from(n) + 1),
        }
    };
    Ok(Image {
        width: canvas.width,
        height: canvas.height,
        frames,
        plays,
    })
}

struct Rect {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

/// premultiplied rgba8. gif transparency is all or nothing, so straight
/// opaque pixels and cleared transparent ones are already premultiplied.
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    /// the part of `rect` inside the canvas, as x and y ranges. frames may
    /// hang off the logical screen; that part is never shown.
    fn clip(&self, rect: &Rect) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let x0 = rect.left.min(self.width) as usize;
        let y0 = rect.top.min(self.height) as usize;
        let x1 = (rect.left + rect.width).min(self.width) as usize;
        let y1 = (rect.top + rect.height).min(self.height) as usize;
        (x0..x1, y0..y1)
    }

    /// draws the frame's rgba pixels; transparent ones leave the canvas alone.
    fn draw(&mut self, rect: &Rect, src: &[u8]) {
        let (xs, ys) = self.clip(rect);
        let src_stride = rect.width as usize * 4;
        for y in ys {
            let src_row = (y - rect.top as usize) * src_stride;
            let dst_row = y * self.width as usize * 4;
            for x in xs.clone() {
                let s = src_row + (x - rect.left as usize) * 4;
                let Some(px) = src.get(s..s + 4) else {
                    return;
                };
                if px[3] != 0 {
                    let d = dst_row + x * 4;
                    self.pixels[d..d + 4].copy_from_slice(px);
                }
            }
        }
    }

    fn clear(&mut self, rect: &Rect) {
        let (xs, ys) = self.clip(rect);
        for y in ys {
            let row = y * self.width as usize * 4;
            self.pixels[row + xs.start * 4..row + xs.end * 4].fill(0);
        }
    }
}
