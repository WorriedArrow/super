//! webp decoding per https://www.rfc-editor.org/rfc/rfc9649. image-webp does the
//! canvas assembly of section 2.7.2.

use std::io::Cursor;
use std::time::Duration;

use image_webp::{LoopCount, WebPDecoder};

use crate::pixels::{self, Layout};
use crate::{DecodeError, Frame, Image, Limits, Plays, malformed, unimplemented_once};

pub(crate) fn decode(bytes: &[u8], limits: Limits) -> Result<Image, DecodeError> {
    let mut decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(malformed)?;
    let (width, height) = decoder.dimensions();
    limits.check(width, height, 1)?;
    decoder.set_memory_limit(usize::try_from(limits.bytes(4)).unwrap_or(usize::MAX));
    if let Ok(Some(_)) = decoder.icc_profile() {
        unimplemented_once!("icc profiles");
    }

    let layout = if decoder.has_alpha() {
        Layout::Rgba
    } else {
        Layout::Rgb
    };
    let size = decoder
        .output_buffer_size()
        .ok_or(DecodeError::TooLarge { width, height })?;
    let mut buf = vec![0; size];

    if !decoder.is_animated() {
        decoder.read_image(&mut buf).map_err(malformed)?;
        return Ok(Image {
            width,
            height,
            frames: vec![to_frame(&buf, layout, Duration::ZERO)],
            plays: Plays::Times(1),
        });
    }

    // section 2.7.1.1 (ANIM): the background color is a hint viewers are
    // not required to use. image-webp starts from transparent black unless told
    // otherwise, which is what browsers do.
    let mut frames = Vec::new();
    for _ in 0..decoder.num_frames() {
        limits.check(width, height, frames.len() as u64 + 1)?;
        match decoder.read_frame(&mut buf) {
            // section 2.7.1.1 (ANMF): frame duration is in milliseconds.
            Ok(ms) => frames.push(to_frame(&buf, layout, Duration::from_millis(ms.into()))),
            Err(err) if frames.is_empty() => return Err(malformed(err)),
            // a corrupt or truncated frame ends the animation at the last
            // complete frame.
            Err(_) => break,
        }
    }
    if frames.is_empty() {
        return Err(malformed("webp animation has no frames"));
    }
    let plays = match (frames.len(), decoder.loop_count()) {
        (1, _) => Plays::Times(1),
        // section 2.7.1.1 (ANIM): a loop count of 0 loops forever.
        (_, LoopCount::Forever) => Plays::Infinite,
        (_, LoopCount::Times(n)) => Plays::Times(u32::from(n.get())),
    };
    Ok(Image {
        width,
        height,
        frames,
        plays,
    })
}

fn to_frame(buf: &[u8], layout: Layout, delay: Duration) -> Frame {
    let mut rgba = pixels::to_rgba(buf, layout);
    pixels::premultiply(&mut rgba);
    Frame {
        pixels: rgba,
        delay,
    }
}
