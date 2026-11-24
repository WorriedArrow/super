//! image decoding: png, jpeg, gif and webp bytes in, premultiplied srgb rgba8
//! frames out.

mod gif_dec;
mod jpeg_dec;
mod pixels;
mod png_dec;
mod sniff;
mod webp_dec;

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

pub use sniff::{ImageType, match_image_type_pattern};

/// logs `unimplemented: <feature>` the first time it runs in this process.
macro_rules! unimplemented_once {
    ($feature:literal) => {{
        static LOGGED: ::std::sync::atomic::AtomicBool =
            ::std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, ::std::sync::atomic::Ordering::Relaxed) {
            eprintln!(concat!("unimplemented: ", $feature));
        }
    }};
}
pub(crate) use unimplemented_once;

/// a decoded image. every frame covers the whole `width` x `height` canvas,
/// with animation frames already composited onto the frames before them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// one frame for still images, one or more for animations.
    pub frames: Vec<Frame>,
    /// how many times the animation plays. `Times(1)` for still images.
    pub plays: Plays,
}

impl Image {
    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// srgb, premultiplied alpha, rgba8, rows top to bottom with no padding.
    /// length is `width * height * 4`.
    pub pixels: Vec<u8>,
    /// how long the frame stays up, as stored in the file. zero for still
    /// images. clamping tiny delays is up to whoever plays the animation.
    pub delay: Duration,
}

/// the total number of times an animation plays, counting the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plays {
    Infinite,
    Times(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// the most pixels decoded across all frames of one image. checked
    /// against header dimensions before any pixel buffer is allocated.
    pub max_pixels: u64,
}

impl Limits {
    /// 64 mebipixels, 256 MiB of rgba8.
    pub const DEFAULT_MAX_PIXELS: u64 = 1 << 26;

    /// fails with `TooLarge` when `frames` frames of `width` x `height`
    /// would exceed `max_pixels`.
    pub(crate) fn check(&self, width: u32, height: u32, frames: u64) -> Result<(), DecodeError> {
        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|p| p.checked_mul(frames));
        match pixels {
            Some(p) if p <= self.max_pixels => Ok(()),
            _ => Err(DecodeError::TooLarge { width, height }),
        }
    }

    /// `max_pixels` as a byte budget for `bytes_per_pixel`, for decoders that
    /// take their own allocation limit.
    pub(crate) fn bytes(&self, bytes_per_pixel: u64) -> u64 {
        self.max_pixels.saturating_mul(bytes_per_pixel)
    }
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_pixels: Self::DEFAULT_MAX_PIXELS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// the bytes match no image type pattern.
    UnknownFormat,
    /// a recognized format super doesn't decode yet.
    Unsupported(ImageType),
    /// the image is bigger than `Limits` allows.
    TooLarge { width: u32, height: u32 },
    /// corrupt, truncated or otherwise undecodable data.
    Malformed(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::UnknownFormat => f.write_str("unknown image format"),
            DecodeError::Unsupported(ty) => write!(f, "unsupported image format: {}", ty.mime()),
            DecodeError::TooLarge { width, height } => {
                write!(f, "image too large: {width}x{height}")
            }
            DecodeError::Malformed(msg) => write!(f, "malformed image: {msg}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub(crate) fn malformed(err: impl fmt::Display) -> DecodeError {
    DecodeError::Malformed(err.to_string())
}

/// decodes `bytes` with the default limits.
pub fn decode(bytes: &[u8]) -> Result<Image, DecodeError> {
    decode_with_limits(bytes, Limits::default())
}

/// sniffs the format from the bytes with the image type pattern matching
/// algorithm, then decodes it.
pub fn decode_with_limits(bytes: &[u8], limits: Limits) -> Result<Image, DecodeError> {
    let ty = match_image_type_pattern(bytes).ok_or(DecodeError::UnknownFormat)?;
    let decode = || match ty {
        ImageType::Png => png_dec::decode(bytes, limits),
        ImageType::Jpeg => jpeg_dec::decode(bytes, limits),
        ImageType::Gif => gif_dec::decode(bytes, limits),
        ImageType::Webp => webp_dec::decode(bytes, limits),
        ImageType::Bmp => {
            unimplemented_once!("bmp images");
            Err(DecodeError::Unsupported(ty))
        }
        ImageType::Icon => {
            unimplemented_once!("ico images");
            Err(DecodeError::Unsupported(ty))
        }
    };
    // page-controlled bytes must never take the process down, so a bug in a
    // third-party decoder becomes a decode error instead (agents/core.md).
    catch_unwind(AssertUnwindSafe(decode))
        .unwrap_or_else(|_| Err(DecodeError::Malformed("decoder panicked".into())))
}
