use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::{DecodeError, Frame, Image, Limits, Plays, malformed, unimplemented_once};

pub(crate) fn decode(bytes: &[u8], limits: Limits) -> Result<Image, DecodeError> {
    // jpeg dimensions are 16-bit, so lift zune's per-axis caps and let the
    // pixel limit decide. the default flags tolerate recoverable stream
    // errors the way other decoders do.
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGBA)
        .set_max_width(usize::from(u16::MAX))
        .set_max_height(usize::from(u16::MAX));
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(malformed)?;
    let info = decoder
        .info()
        .ok_or_else(|| malformed("jpeg headers missing"))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    limits.check(width, height, 1)?;
    if decoder.icc_profile().is_some() {
        unimplemented_once!("icc profiles");
    }

    let size = decoder
        .output_buffer_size()
        .ok_or(DecodeError::TooLarge { width, height })?;
    if size != width as usize * height as usize * 4 {
        return Err(malformed("jpeg output size mismatch"));
    }
    let mut pixels = vec![0; size];
    decoder.decode_into(&mut pixels).map_err(malformed)?;
    // jpeg has no alpha, so every pixel is already premultiplied.
    Ok(Image {
        width,
        height,
        frames: vec![Frame {
            pixels,
            delay: Default::default(),
        }],
        plays: Plays::Times(1),
    })
}
