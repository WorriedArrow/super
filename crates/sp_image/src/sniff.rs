/// an image mime type the image type pattern matching algorithm can return.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageType {
    /// image/x-icon, covering both windows icons and cursors.
    Icon,
    Bmp,
    Gif,
    Webp,
    Png,
    Jpeg,
}

impl ImageType {
    pub fn mime(self) -> &'static str {
        match self {
            ImageType::Icon => "image/x-icon",
            ImageType::Bmp => "image/bmp",
            ImageType::Gif => "image/gif",
            ImageType::Webp => "image/webp",
            ImageType::Png => "image/png",
            ImageType::Jpeg => "image/jpeg",
        }
    }
}

struct Row {
    pattern: &'static [u8],
    mask: &'static [u8],
    ty: ImageType,
}

/// the table in https://mimesniff.spec.whatwg.org/#image-type-pattern-matching-algorithm.
/// no row ignores leading bytes.
const IMAGE_TYPE_PATTERNS: &[Row] = &[
    // windows icon
    Row {
        pattern: b"\x00\x00\x01\x00",
        mask: b"\xff\xff\xff\xff",
        ty: ImageType::Icon,
    },
    // windows cursor
    Row {
        pattern: b"\x00\x00\x02\x00",
        mask: b"\xff\xff\xff\xff",
        ty: ImageType::Icon,
    },
    Row {
        pattern: b"BM",
        mask: b"\xff\xff",
        ty: ImageType::Bmp,
    },
    Row {
        pattern: b"GIF87a",
        mask: b"\xff\xff\xff\xff\xff\xff",
        ty: ImageType::Gif,
    },
    Row {
        pattern: b"GIF89a",
        mask: b"\xff\xff\xff\xff\xff\xff",
        ty: ImageType::Gif,
    },
    // "RIFF" followed by four bytes followed by "WEBPVP"
    Row {
        pattern: b"RIFF\x00\x00\x00\x00WEBPVP",
        mask: b"\xff\xff\xff\xff\x00\x00\x00\x00\xff\xff\xff\xff\xff\xff",
        ty: ImageType::Webp,
    },
    Row {
        pattern: b"\x89PNG\r\n\x1a\x0a",
        mask: b"\xff\xff\xff\xff\xff\xff\xff\xff",
        ty: ImageType::Png,
    },
    // the jpeg start of image marker followed by the indicator byte of another marker
    Row {
        pattern: b"\xff\xd8\xff",
        mask: b"\xff\xff\xff",
        ty: ImageType::Jpeg,
    },
];

/// https://mimesniff.spec.whatwg.org/#image-type-pattern-matching-algorithm
pub fn match_image_type_pattern(input: &[u8]) -> Option<ImageType> {
    // step 1: for each row, run the pattern matching algorithm and return the
    // row's type on a match.
    // step 2: return undefined.
    IMAGE_TYPE_PATTERNS
        .iter()
        .find(|row| pattern_matching_algorithm(input, row.pattern, row.mask, &[]))
        .map(|row| row.ty)
}

/// https://mimesniff.spec.whatwg.org/#pattern-matching-algorithm
fn pattern_matching_algorithm(input: &[u8], pattern: &[u8], mask: &[u8], ignored: &[u8]) -> bool {
    // step 1
    debug_assert_eq!(pattern.len(), mask.len());
    // step 2
    if input.len() < pattern.len() {
        return false;
    }
    // steps 3-4: skip leading ignored bytes.
    let s = input
        .iter()
        .position(|b| !ignored.contains(b))
        .unwrap_or(input.len());
    let input = &input[s..];
    if input.len() < pattern.len() {
        return false;
    }
    // steps 5-6
    input
        .iter()
        .zip(pattern.iter().zip(mask))
        .all(|(&byte, (&p, &m))| byte & m == p)
    // step 7: return true.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        let cases: &[(&[u8], Option<ImageType>)] = &[
            (b"\x89PNG\r\n\x1a\x0a....", Some(ImageType::Png)),
            (b"\xff\xd8\xff\xe0", Some(ImageType::Jpeg)),
            (b"GIF87a", Some(ImageType::Gif)),
            (b"GIF89a\x01\x00", Some(ImageType::Gif)),
            (b"RIFF\x10\x20\x30\x40WEBPVP8 ", Some(ImageType::Webp)),
            (b"RIFF\x10\x20\x30\x40WEBPVP8L", Some(ImageType::Webp)),
            (b"BM\x00", Some(ImageType::Bmp)),
            (b"\x00\x00\x01\x00", Some(ImageType::Icon)),
            (b"\x00\x00\x02\x00", Some(ImageType::Icon)),
            (b"", None),
            (b"\x89PNG\r\n\x1a", None),
            (b"\xff\xd8", None),
            (b"GIF88a", None),
            (b"RIFF\x10\x20\x30\x40WAVEfmt ", None),
            (b"RIFF\x10\x20\x30\x40WEBPXX", None),
            (b"<svg", None),
        ];
        for (bytes, want) in cases {
            assert_eq!(match_image_type_pattern(bytes), *want, "{bytes:?}");
        }
    }

    #[test]
    fn ignored_leading_bytes_are_skipped() {
        assert!(pattern_matching_algorithm(
            b"  ab",
            b"ab",
            b"\xff\xff",
            b" "
        ));
        assert!(!pattern_matching_algorithm(
            b"  ab",
            b"ab",
            b"\xff\xff",
            b""
        ));
        assert!(!pattern_matching_algorithm(
            b"   a",
            b"ab",
            b"\xff\xff",
            b" "
        ));
    }
}
