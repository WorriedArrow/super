//! pixel format conversions shared by the decoders.

/// channel layouts the decoders hand back, 8 bits per sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}

impl Layout {
    pub(crate) fn channels(self) -> usize {
        match self {
            Layout::Gray => 1,
            Layout::GrayAlpha => 2,
            Layout::Rgb => 3,
            Layout::Rgba => 4,
        }
    }
}

/// expands `src` in `layout` to straight-alpha rgba8.
pub(crate) fn to_rgba(src: &[u8], layout: Layout) -> Vec<u8> {
    match layout {
        Layout::Rgba => src.to_vec(),
        Layout::Rgb => src
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        Layout::GrayAlpha => src
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        Layout::Gray => src.iter().flat_map(|&g| [g, g, g, 255]).collect(),
    }
}

/// multiplies the color channels of straight-alpha rgba8 by alpha, rounding
/// to nearest.
pub(crate) fn premultiply(rgba: &mut [u8]) {
    for [r, g, b, a] in rgba.as_chunks_mut::<4>().0 {
        if *a == 255 {
            continue;
        }
        let alpha = u16::from(*a);
        for c in [r, g, b] {
            *c = mul_div_255(u16::from(*c), alpha);
        }
    }
}

/// round(x * a / 255) for x, a in 0..=255, without division.
fn mul_div_255(x: u16, a: u16) -> u8 {
    let t = x * a + 128;
    ((t + (t >> 8)) >> 8) as u8
}

/// reduces big-endian 16-bit samples to 8 bits, rounding to nearest.
/// https://www.w3.org/TR/png-3/#13Sample-depth-rescaling
pub(crate) fn sixteen_to_eight(src: &[u8]) -> Vec<u8> {
    src.as_chunks::<2>()
        .0
        .iter()
        .map(|&s| {
            let v = u32::from(u16::from_be_bytes(s));
            ((v * 255 + 32767) / 65535) as u8
        })
        .collect()
}

/// a lookup table applying png decoder gamma correction to 8-bit samples
/// whose gAMA value is `file_gamma`, for an srgb display.
///
/// https://www.w3.org/TR/png-3/#13Decoder-gamma-handling:
/// display_input = sample ^ (1 / (gamma * display_exponent)), with the
/// recommended display exponent of 2.2, and
/// framebuf_sample = floor(display_input * max + 0.5).
pub(crate) fn gamma_table(file_gamma: f64) -> [u8; 256] {
    const DISPLAY_EXPONENT: f64 = 2.2;
    let decoding_exponent = 1.0 / (file_gamma * DISPLAY_EXPONENT);
    let mut table = [0u8; 256];
    for (i, out) in table.iter_mut().enumerate() {
        let display_input = (i as f64 / 255.0).powf(decoding_exponent);
        *out = (display_input * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_rounds() {
        let mut px = [255, 128, 0, 128, 10, 20, 30, 0, 1, 2, 3, 255];
        premultiply(&mut px);
        assert_eq!(px, [128, 64, 0, 128, 0, 0, 0, 0, 1, 2, 3, 255]);
    }

    #[test]
    fn mul_div_255_is_exact() {
        for x in 0..=255u16 {
            for a in 0..=255u16 {
                let want = ((f64::from(x) * f64::from(a)) / 255.0).round() as u8;
                assert_eq!(mul_div_255(x, a), want, "{x} * {a}");
            }
        }
    }

    #[test]
    fn layouts_expand() {
        assert_eq!(to_rgba(&[7], Layout::Gray), [7, 7, 7, 255]);
        assert_eq!(to_rgba(&[7, 9], Layout::GrayAlpha), [7, 7, 7, 9]);
        assert_eq!(to_rgba(&[1, 2, 3], Layout::Rgb), [1, 2, 3, 255]);
    }

    #[test]
    fn sixteen_bit_rounds() {
        assert_eq!(
            sixteen_to_eight(&[0x00, 0x00, 0xff, 0xff, 0x80, 0x00, 0x01, 0x01]),
            [0, 255, 128, 1]
        );
    }

    #[test]
    fn gamma_tables() {
        // linear samples (gamma 1.0) at 50% light: (128/255)^(1/2.2) = 0.731.
        let linear = gamma_table(1.0);
        assert_eq!((linear[0], linear[128], linear[255]), (0, 186, 255));
        // gamma 1/2.2 already matches the display.
        let matched = gamma_table(1.0 / 2.2);
        assert!(
            matched
                .iter()
                .enumerate()
                .all(|(i, &v)| usize::from(v) == i)
        );
    }
}
