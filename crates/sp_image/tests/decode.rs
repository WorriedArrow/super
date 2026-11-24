use std::borrow::Cow;
use std::time::Duration;

use sp_image::{DecodeError, Image, ImageType, Limits, Plays, decode, decode_with_limits};

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const CLEAR: [u8; 4] = [0, 0, 0, 0];

const BASELINE_JPG: &[u8] = include_bytes!("fixtures/baseline.jpg");
const PROGRESSIVE_JPG: &[u8] = include_bytes!("fixtures/progressive.jpg");
const GRAY_JPG: &[u8] = include_bytes!("fixtures/gray.jpg");
const LOSSY_WEBP: &[u8] = include_bytes!("fixtures/lossy.webp");
const ANIMATED_LOSSY_WEBP: &[u8] = include_bytes!("fixtures/animated_lossy.webp");

fn pixel(image: &Image, frame: usize, x: u32, y: u32) -> [u8; 4] {
    let i = (y * image.width + x) as usize * 4;
    image.frames[frame].pixels[i..i + 4].try_into().unwrap()
}

fn assert_close(got: [u8; 4], want: [u8; 4], tolerance: u8) {
    for (g, w) in got.iter().zip(want) {
        assert!(g.abs_diff(w) <= tolerance, "got {got:?}, want {want:?}");
    }
}

fn assert_well_formed(image: &Image) {
    assert!(!image.frames.is_empty());
    for frame in &image.frames {
        assert_eq!(
            frame.pixels.len(),
            image.width as usize * image.height as usize * 4
        );
        for p in frame.pixels.as_chunks::<4>().0 {
            assert!(
                p[..3].iter().all(|&c| c <= p[3]),
                "not premultiplied: {p:?}"
            );
        }
    }
}

// png

fn encode_png(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
    setup: impl FnOnce(&mut png::Encoder<'_, &mut Vec<u8>>),
) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    setup(&mut encoder);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    out
}

#[test]
fn png_rgba8_is_premultiplied() {
    let data = [
        255, 0, 0, 255, 255, 255, 255, 128, 0, 0, 255, 0, 10, 20, 30, 51,
    ];
    let png = encode_png(
        2,
        2,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &data,
        |_| {},
    );
    let image = decode(&png).unwrap();
    assert_eq!((image.width, image.height, image.frames.len()), (2, 2, 1));
    assert_eq!(image.plays, Plays::Times(1));
    assert_eq!(image.frames[0].delay, Duration::ZERO);
    assert_eq!(
        image.frames[0].pixels,
        [255, 0, 0, 255, 128, 128, 128, 128, 0, 0, 0, 0, 2, 4, 6, 51]
    );
}

#[test]
fn png_16_bit_rounds_to_8() {
    // rgb, 16 bits per sample, big endian.
    let data = [
        0xff, 0xff, 0x80, 0x00, 0x00, 0x00, //
        0x01, 0x01, 0x00, 0x80, 0xfe, 0xff,
    ];
    let png = encode_png(
        2,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Sixteen,
        &data,
        |_| {},
    );
    let image = decode(&png).unwrap();
    assert_eq!(image.frames[0].pixels, [255, 128, 0, 255, 1, 0, 254, 255]);
}

#[test]
fn png_16_bit_gray_alpha() {
    let data = [0xff, 0xff, 0x80, 0x00];
    let png = encode_png(
        1,
        1,
        png::ColorType::GrayscaleAlpha,
        png::BitDepth::Sixteen,
        &data,
        |_| {},
    );
    assert_eq!(decode(&png).unwrap().frames[0].pixels, [128, 128, 128, 128]);
}

#[test]
fn png_palette_with_trns() {
    // 2-bit palette indices: 0, 1, 2, 1 packed into one byte.
    let png = encode_png(
        4,
        1,
        png::ColorType::Indexed,
        png::BitDepth::Two,
        &[0b00_01_10_01],
        |e| {
            e.set_palette(vec![255, 0, 0, 0, 255, 0, 0, 0, 255]);
            e.set_trns(vec![255, 0]);
        },
    );
    let image = decode(&png).unwrap();
    assert_eq!(pixel(&image, 0, 0, 0), RED);
    assert_eq!(pixel(&image, 0, 1, 0), CLEAR);
    assert_eq!(pixel(&image, 0, 2, 0), BLUE);
}

#[test]
fn png_gray_1_bit() {
    let png = encode_png(
        8,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::One,
        &[0b1000_0001],
        |_| {},
    );
    let image = decode(&png).unwrap();
    assert_eq!(pixel(&image, 0, 0, 0), [255, 255, 255, 255]);
    assert_eq!(pixel(&image, 0, 1, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&image, 0, 7, 0), [255, 255, 255, 255]);
}

#[test]
fn png_gama_converts_to_srgb() {
    // gamma 1.0 means linear samples: (128/255)^(1/2.2) = 0.731.
    let png = encode_png(
        1,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[128, 0, 255],
        |e| e.set_source_gamma(png::ScaledFloat::new(1.0)),
    );
    assert_eq!(decode(&png).unwrap().frames[0].pixels, [186, 0, 255, 255]);
}

#[test]
fn png_srgb_chunk_overrides_gama() {
    let png = encode_png(
        1,
        1,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &[128, 0, 255],
        |e| {
            e.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
            e.set_source_gamma(png::ScaledFloat::new(1.0));
        },
    );
    assert_eq!(decode(&png).unwrap().frames[0].pixels, [128, 0, 255, 255]);
}

#[test]
fn png_gama_leaves_alpha_alone() {
    let png = encode_png(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[128, 128, 128, 128],
        |e| e.set_source_gamma(png::ScaledFloat::new(1.0)),
    );
    // 186 premultiplied by 128/255.
    assert_eq!(decode(&png).unwrap().frames[0].pixels, [93, 93, 93, 128]);
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

#[test]
fn png_hostile_header_is_rejected_before_allocating() {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&1_000_000u32.to_be_bytes());
    ihdr.extend_from_slice(&1_000_000u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(
        &mut png,
        b"IDAT",
        &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01],
    );
    png_chunk(&mut png, b"IEND", &[]);
    assert_eq!(
        decode(&png),
        Err(DecodeError::TooLarge {
            width: 1_000_000,
            height: 1_000_000
        })
    );
}

#[test]
fn png_truncated() {
    let data: Vec<u8> = (0..16 * 16 * 3).map(|i| (i * 7) as u8).collect();
    let png = encode_png(
        16,
        16,
        png::ColorType::Rgb,
        png::BitDepth::Eight,
        &data,
        |_| {},
    );
    assert!(decode(&png).is_ok());
    assert!(matches!(
        decode(&png[..png.len() - 20]),
        Err(DecodeError::Malformed(_))
    ));
    assert!(matches!(decode(&png[..20]), Err(DecodeError::Malformed(_))));
}

// jpeg

#[test]
fn jpeg_baseline() {
    let image = decode(BASELINE_JPG).unwrap();
    assert_eq!((image.width, image.height, image.frames.len()), (16, 16, 1));
    assert_well_formed(&image);
    assert_close(pixel(&image, 0, 3, 8), RED, 8);
    assert_close(pixel(&image, 0, 12, 8), BLUE, 8);
}

#[test]
fn jpeg_progressive() {
    let image = decode(PROGRESSIVE_JPG).unwrap();
    assert_eq!((image.width, image.height), (16, 16));
    assert_well_formed(&image);
    assert_close(pixel(&image, 0, 3, 8), RED, 8);
    assert_close(pixel(&image, 0, 12, 8), BLUE, 8);
}

#[test]
fn jpeg_grayscale() {
    let image = decode(GRAY_JPG).unwrap();
    assert_well_formed(&image);
    assert_close(pixel(&image, 0, 5, 5), [128, 128, 128, 255], 2);
}

#[test]
fn jpeg_truncated() {
    // cut inside the frame header.
    assert!(matches!(
        decode(&BASELINE_JPG[..30]),
        Err(DecodeError::Malformed(_))
    ));
}

#[test]
fn jpeg_over_limit() {
    let limits = Limits { max_pixels: 255 };
    assert_eq!(
        decode_with_limits(BASELINE_JPG, limits),
        Err(DecodeError::TooLarge {
            width: 16,
            height: 16
        })
    );
    assert!(decode_with_limits(BASELINE_JPG, Limits { max_pixels: 256 }).is_ok());
}

// gif

/// global palette: 0 red, 1 blue, 2 green, 3 transparent.
const GIF_PALETTE: &[u8] = &[255, 0, 0, 0, 0, 255, 0, 255, 0, 0, 0, 0];

fn gif_frame(
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    index: u8,
    dispose: gif::DisposalMethod,
    delay: u16,
) -> gif::Frame<'static> {
    gif::Frame {
        left,
        top,
        width,
        height,
        dispose,
        delay,
        transparent: Some(3),
        buffer: Cow::Owned(vec![index; usize::from(width) * usize::from(height)]),
        ..Default::default()
    }
}

fn encode_gif(
    width: u16,
    height: u16,
    repeat: Option<gif::Repeat>,
    frames: &[gif::Frame<'_>],
) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, width, height, GIF_PALETTE).unwrap();
        if let Some(repeat) = repeat {
            encoder.set_repeat(repeat).unwrap();
        }
        for frame in frames {
            encoder.write_frame(frame).unwrap();
        }
    }
    out
}

#[test]
fn gif_animation_composites_frames() {
    use gif::DisposalMethod::{Background, Keep, Previous};
    let gif = encode_gif(
        4,
        4,
        Some(gif::Repeat::Infinite),
        &[
            gif_frame(0, 0, 4, 4, 0, Keep, 10),
            gif_frame(1, 1, 2, 2, 1, Background, 20),
            gif_frame(3, 3, 1, 1, 2, Previous, 0),
            gif_frame(0, 0, 1, 1, 3, Keep, 5),
        ],
    );
    let image = decode(&gif).unwrap();
    assert_well_formed(&image);
    assert_eq!((image.width, image.height, image.frames.len()), (4, 4, 4));
    assert!(image.is_animated());
    assert_eq!(image.plays, Plays::Infinite);
    let delays: Vec<_> = image.frames.iter().map(|f| f.delay).collect();
    assert_eq!(
        delays,
        [100, 200, 0, 50].map(Duration::from_millis).to_vec()
    );

    // frame 0: all red.
    assert!(image.frames[0].pixels.chunks(4).all(|p| p == RED));
    // frame 1: blue square drawn over red.
    assert_eq!(pixel(&image, 1, 0, 0), RED);
    assert_eq!(pixel(&image, 1, 1, 1), BLUE);
    assert_eq!(pixel(&image, 1, 2, 2), BLUE);
    assert_eq!(pixel(&image, 1, 3, 3), RED);
    // frame 2: the blue square was restored to the transparent background,
    // then green drawn in the corner.
    assert_eq!(pixel(&image, 2, 1, 1), CLEAR);
    assert_eq!(pixel(&image, 2, 2, 2), CLEAR);
    assert_eq!(pixel(&image, 2, 3, 3), GREEN);
    assert_eq!(pixel(&image, 2, 0, 0), RED);
    // frame 3: the green was restored to previous and the transparent pixel
    // drawn at 0,0 left red showing.
    assert_eq!(pixel(&image, 3, 3, 3), RED);
    assert_eq!(pixel(&image, 3, 0, 0), RED);
    assert_eq!(pixel(&image, 3, 1, 1), CLEAR);
}

#[test]
fn gif_loop_counts() {
    use gif::DisposalMethod::Keep;
    let frames = [
        gif_frame(0, 0, 1, 1, 0, Keep, 1),
        gif_frame(0, 0, 1, 1, 1, Keep, 1),
    ];
    let plays = |repeat| decode(&encode_gif(1, 1, repeat, &frames)).unwrap().plays;
    assert_eq!(plays(None), Plays::Times(1));
    assert_eq!(plays(Some(gif::Repeat::Finite(2))), Plays::Times(3));
    assert_eq!(plays(Some(gif::Repeat::Infinite)), Plays::Infinite);
}

#[test]
fn gif_frame_outside_screen_is_clipped() {
    let gif = encode_gif(
        2,
        2,
        None,
        &[gif_frame(1, 1, 3, 3, 1, gif::DisposalMethod::Keep, 0)],
    );
    let image = decode(&gif).unwrap();
    assert_eq!((image.width, image.height), (2, 2));
    assert_eq!(pixel(&image, 0, 0, 0), CLEAR);
    assert_eq!(pixel(&image, 0, 1, 1), BLUE);
    assert!(!image.is_animated());
}

#[test]
fn gif_zero_screen_uses_first_frame() {
    let gif = encode_gif(
        0,
        0,
        None,
        &[gif_frame(1, 0, 2, 3, 2, gif::DisposalMethod::Keep, 0)],
    );
    let image = decode(&gif).unwrap();
    assert_eq!((image.width, image.height), (3, 3));
    assert_eq!(pixel(&image, 0, 0, 0), CLEAR);
    assert_eq!(pixel(&image, 0, 2, 2), GREEN);
}

#[test]
fn gif_truncated() {
    use gif::DisposalMethod::Keep;
    let first = gif_frame(0, 0, 8, 8, 0, Keep, 3);
    let second = gif_frame(0, 0, 8, 8, 1, Keep, 3);
    let one = encode_gif(
        8,
        8,
        Some(gif::Repeat::Infinite),
        std::slice::from_ref(&first),
    );
    let two = encode_gif(8, 8, Some(gif::Repeat::Infinite), &[first, second]);
    // drop the trailer from the one-frame file to find where frame two starts.
    let second_start = one.len() - 1;
    let cut = &two[..second_start + 12];
    let image = decode(cut).unwrap();
    assert_eq!(image.frames.len(), 1);
    assert!(image.frames[0].pixels.chunks(4).all(|p| p == RED));
    // cut inside the first frame.
    assert!(matches!(decode(&two[..30]), Err(DecodeError::Malformed(_))));
    // header only.
    assert!(matches!(decode(&two[..8]), Err(DecodeError::Malformed(_))));
}

#[test]
fn gif_frames_count_against_limit() {
    use gif::DisposalMethod::Keep;
    let frames = [
        gif_frame(0, 0, 4, 4, 0, Keep, 1),
        gif_frame(0, 0, 4, 4, 1, Keep, 1),
    ];
    let gif = encode_gif(4, 4, None, &frames);
    assert!(decode_with_limits(&gif, Limits { max_pixels: 32 }).is_ok());
    assert_eq!(
        decode_with_limits(&gif, Limits { max_pixels: 31 }),
        Err(DecodeError::TooLarge {
            width: 4,
            height: 4
        })
    );
}

// webp

#[test]
fn webp_lossless() {
    let data = [
        255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 0, 200, 100, 50, 51,
    ];
    let mut webp = Vec::new();
    image_webp::WebPEncoder::new(&mut webp)
        .encode(&data, 2, 2, image_webp::ColorType::Rgba8)
        .unwrap();
    let image = decode(&webp).unwrap();
    assert_eq!((image.width, image.height, image.frames.len()), (2, 2, 1));
    assert_eq!(
        image.frames[0].pixels,
        [255, 0, 0, 255, 0, 128, 0, 128, 0, 0, 0, 0, 40, 20, 10, 51]
    );
}

#[test]
fn webp_lossless_opaque_rgb() {
    let data = [1, 2, 3, 4, 5, 6];
    let mut webp = Vec::new();
    image_webp::WebPEncoder::new(&mut webp)
        .encode(&data, 2, 1, image_webp::ColorType::Rgb8)
        .unwrap();
    assert_eq!(
        decode(&webp).unwrap().frames[0].pixels,
        [1, 2, 3, 255, 4, 5, 6, 255]
    );
}

#[test]
fn webp_lossy() {
    let image = decode(LOSSY_WEBP).unwrap();
    assert_eq!((image.width, image.height, image.frames.len()), (16, 16, 1));
    assert_well_formed(&image);
    assert_close(pixel(&image, 0, 3, 8), RED, 12);
    assert_close(pixel(&image, 0, 12, 8), BLUE, 12);
}

#[test]
fn webp_animated() {
    let image = decode(ANIMATED_LOSSY_WEBP).unwrap();
    assert_well_formed(&image);
    assert_eq!((image.width, image.height, image.frames.len()), (16, 16, 2));
    assert_eq!(image.plays, Plays::Infinite);
    assert_eq!(image.frames[0].delay, Duration::from_millis(100));
    assert_eq!(image.frames[1].delay, Duration::from_millis(250));
    assert_close(pixel(&image, 0, 8, 8), GREEN, 12);
    assert_close(pixel(&image, 1, 8, 8), BLUE, 12);
}

#[test]
fn webp_truncated() {
    assert!(matches!(
        decode(&LOSSY_WEBP[..LOSSY_WEBP.len() / 2]),
        Err(DecodeError::Malformed(_))
    ));
}

// sniffing and robustness

#[test]
fn format_comes_from_bytes() {
    assert_eq!(decode(b"not an image"), Err(DecodeError::UnknownFormat));
    assert_eq!(decode(b""), Err(DecodeError::UnknownFormat));
    assert_eq!(
        decode(b"BM\x00\x00\x00\x00"),
        Err(DecodeError::Unsupported(ImageType::Bmp))
    );
    assert_eq!(
        decode(b"\x00\x00\x01\x00\x01\x00"),
        Err(DecodeError::Unsupported(ImageType::Icon))
    );
}

fn corpus() -> Vec<Vec<u8>> {
    use gif::DisposalMethod::{Background, Keep};
    let mut lossless = Vec::new();
    image_webp::WebPEncoder::new(&mut lossless)
        .encode(&[9; 4 * 4 * 4], 4, 4, image_webp::ColorType::Rgba8)
        .unwrap();
    vec![
        BASELINE_JPG.to_vec(),
        PROGRESSIVE_JPG.to_vec(),
        GRAY_JPG.to_vec(),
        LOSSY_WEBP.to_vec(),
        ANIMATED_LOSSY_WEBP.to_vec(),
        lossless,
        encode_png(
            3,
            3,
            png::ColorType::Rgba,
            png::BitDepth::Sixteen,
            &[0x55; 3 * 3 * 8],
            |_| {},
        ),
        encode_gif(
            4,
            4,
            Some(gif::Repeat::Infinite),
            &[
                gif_frame(0, 0, 4, 4, 0, Keep, 1),
                gif_frame(1, 1, 2, 2, 1, Background, 1),
            ],
        ),
    ]
}

/// every prefix of every fixture, and every fixture with each byte flipped,
/// decodes or fails without a decoder panicking.
#[test]
fn corrupt_input_never_panics() {
    let panicked = Err(DecodeError::Malformed("decoder panicked".into()));
    for file in corpus() {
        for len in 0..file.len() {
            let result = decode(&file[..len]);
            assert_ne!(result, panicked, "prefix {len} of {:?}", &file[..8]);
            if let Ok(image) = result {
                assert_well_formed(&image);
            }
        }
        for i in 0..file.len() {
            let mut bad = file.clone();
            bad[i] ^= 0xff;
            let result = decode(&bad);
            assert_ne!(result, panicked, "byte {i} flipped in {:?}", &file[..8]);
            if let Ok(image) = result {
                assert_well_formed(&image);
            }
        }
    }
}
