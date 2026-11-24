use std::io::Cursor;

use png::{BitDepth, ColorType, Decoder, Transformations};

use crate::pixels::{self, Layout};
use crate::{DecodeError, Frame, Image, Limits, Plays, malformed, unimplemented_once};

pub(crate) fn decode(bytes: &[u8], limits: Limits) -> Result<Image, DecodeError> {
    // the png crate counts its own allocations; give it room for a 16-bit
    // rgba frame plus a row of slack before our pixel limit takes over.
    let mut decoder = Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: usize::try_from(limits.bytes(8)).unwrap_or(usize::MAX),
        },
    );
    // expand palettes, low bit depths and tRNS to full channels.
    decoder.set_transformations(Transformations::EXPAND);
    let info = decoder.read_header_info().map_err(malformed)?;
    limits.check(info.width, info.height, 1)?;

    let mut reader = decoder.read_info().map_err(malformed)?;
    let size = reader.output_buffer_size().ok_or(DecodeError::TooLarge {
        width: reader.info().width,
        height: reader.info().height,
    })?;
    let mut buf = vec![0; size];
    let out = reader.next_frame(&mut buf).map_err(malformed)?;
    buf.truncate(out.buffer_size());

    let info = reader.info();
    if info.animation_control.is_some() {
        // the default image is what a decoder without apng support shows.
        unimplemented_once!("apng animation");
    }
    let samples = match out.bit_depth {
        BitDepth::Sixteen => pixels::sixteen_to_eight(&buf),
        BitDepth::Eight => buf,
        // EXPAND raises everything below 8 bits to 8.
        _ => return Err(malformed("unexpected png output bit depth")),
    };
    let layout = match out.color_type {
        ColorType::Grayscale => Layout::Gray,
        ColorType::GrayscaleAlpha => Layout::GrayAlpha,
        ColorType::Rgb => Layout::Rgb,
        ColorType::Rgba => Layout::Rgba,
        ColorType::Indexed => return Err(malformed("unexpected png indexed output")),
    };
    if samples.len() != out.width as usize * out.height as usize * layout.channels() {
        return Err(malformed("png output size mismatch"));
    }
    let mut rgba = pixels::to_rgba(&samples, layout);
    if let Some(table) = srgb_table(info) {
        for [r, g, b, _] in rgba.as_chunks_mut::<4>().0 {
            for c in [r, g, b] {
                *c = table[usize::from(*c)];
            }
        }
    }
    pixels::premultiply(&mut rgba);

    Ok(Image {
        width: out.width,
        height: out.height,
        frames: vec![Frame {
            pixels: rgba,
            delay: Default::default(),
        }],
        plays: Plays::Times(1),
    })
}

/// picks the color space chunk that applies and returns the table converting
/// its samples to srgb, or none when they already are srgb.
///
/// https://www.w3.org/TR/png-3/#4Concepts.ColourSpaces: cICP, then iCCP, then
/// sRGB, then gAMA and cHRM. the lower-priority chunks are ignored when a
/// higher one is present.
fn srgb_table(info: &png::Info<'_>) -> Option<[u8; 256]> {
    if info.coding_independent_code_points.is_some() {
        unimplemented_once!("png cICP");
        return None;
    }
    if info.icc_profile.is_some() {
        unimplemented_once!("icc profiles");
        return None;
    }
    // https://www.w3.org/TR/png-3/#srgb-standard-colour-space: samples are srgb.
    if info.srgb.is_some() {
        return None;
    }
    if info.chrm_chunk.is_some() {
        unimplemented_once!("png cHRM");
    }
    // https://www.w3.org/TR/png-3/#11gAMA and #13Decoder-gamma-handling:
    // sample = light ^ gamma. a zero
    // gamma is invalid and the chunk is ignored.
    let gamma = f64::from(info.gama_chunk?.into_value());
    if gamma <= 0.0 {
        return None;
    }
    Some(pixels::gamma_table(gamma))
}
