use super::*;

/// A plane of 8-bit samples, `width` wide.
fn bytes(samples: &[u8], width: usize) -> Plane<'_> {
    Plane::new(samples, width, width, samples.len() / width, false).expect("a plane")
}

/// A plane of 10-bit samples, written as two bytes each.
fn wide(samples: &[u16], width: usize) -> (Vec<u8>, usize, usize) {
    let data = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    (data, width, samples.len() / width)
}

fn yuv<'a>(
    sampling: Sampling,
    full: bool,
    matrix: Option<(f32, f32)>,
    luma: Plane<'a>,
    chroma: Option<(Plane<'a>, Plane<'a>)>,
) -> Yuv<'a> {
    Yuv {
        width: luma.width,
        height: luma.height,
        sampling,
        bits: if luma.wide { 10 } else { 8 },
        full,
        matrix,
        luma,
        chroma,
    }
}

const BT601: Option<(f32, f32)> = Some((0.299, 0.114));
const BT709: Option<(f32, f32)> = Some((0.2126, 0.0722));

fn near(got: &[u8], want: [u8; 3], slack: u8) {
    for (got, want) in got.iter().zip(want) {
        assert!(
            got.abs_diff(want) <= slack,
            "{got:?} is not near {want:?} (±{slack})"
        );
    }
}

#[test]
fn a_file_is_avif_by_the_brands_of_its_first_box() {
    // size, `ftyp`, major brand, minor version, compatible brands.
    let boxed = |major: &[u8; 4], others: &[&[u8; 4]]| {
        let mut out = Vec::new();
        out.extend(((16 + others.len() * 4) as u32).to_be_bytes());
        out.extend(b"ftyp");
        out.extend(major);
        out.extend(0u32.to_be_bytes());
        for brand in others {
            out.extend(*brand);
        }
        out
    };
    assert!(is_avif(&boxed(b"avif", &[])));
    assert!(is_avif(&boxed(b"avis", &[b"avif", b"miaf"])));
    assert!(is_avif(&boxed(b"mif1", &[b"miaf", b"avif"])));
    assert!(!is_avif(&boxed(b"mp42", &[b"isom", b"mp41"])));
    assert!(!is_avif(&boxed(b"heic", &[b"mif1"])));
    assert!(!is_avif(b"GIF89a....ftyp"));
    assert!(!is_avif(b""));
    assert!(!is_avif(b"\x00\x00\x00"));
}

#[test]
fn full_range_planes_become_the_colours_they_were_made_from() {
    // Red, green, blue and white, as BT.601 writes them at full range.
    let luma = [76, 150, 29, 255];
    let blue_difference = [85, 44, 255, 128];
    let red_difference = [255, 21, 107, 128];
    let picture = yuv(
        Sampling::Full,
        true,
        BT601,
        bytes(&luma, 4),
        Some((bytes(&blue_difference, 4), bytes(&red_difference, 4))),
    );
    let out = picture.to_rgba();
    assert_eq!((out.width, out.height), (4, 1));
    near(&out.rgba[0..3], [255, 0, 0], 3);
    near(&out.rgba[4..7], [0, 255, 0], 3);
    near(&out.rgba[8..11], [0, 0, 255], 3);
    near(&out.rgba[12..15], [255, 255, 255], 1);
    assert!(
        out.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255)
    );
}

#[test]
fn video_range_puts_black_at_16_and_white_at_235() {
    let luma = [16, 126, 235, 0, 255];
    let grey = [128; 5];
    let picture = yuv(
        Sampling::Full,
        false,
        BT709,
        bytes(&luma, 5),
        Some((bytes(&grey, 5), bytes(&grey, 5))),
    );
    let out = picture.to_rgba();
    near(&out.rgba[0..3], [0, 0, 0], 1);
    near(&out.rgba[4..7], [128, 128, 128], 2);
    near(&out.rgba[8..11], [255, 255, 255], 1);
    // Past the range, the colour is held to the ends of its own.
    near(&out.rgba[12..15], [0, 0, 0], 0);
    near(&out.rgba[16..19], [255, 255, 255], 0);
}

#[test]
fn a_picture_with_half_its_chroma_is_stretched_and_odd_sizes_hold() {
    // 3 by 3 pixels, so the chroma planes are 2 by 2: all the same colour,
    // which every pixel must show whatever its place, edges included.
    let luma = [120u8; 9];
    let blue_difference = [160u8; 4];
    let red_difference = [100u8; 4];
    let picture = yuv(
        Sampling::Half,
        true,
        BT601,
        Plane::new(&luma, 3, 3, 3, false).unwrap(),
        Some((
            Plane::new(&blue_difference, 2, 2, 2, false).unwrap(),
            Plane::new(&red_difference, 2, 2, 2, false).unwrap(),
        )),
    );
    let out = picture.to_rgba();
    let first: Vec<u8> = out.rgba[0..4].to_vec();
    assert!(
        out.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel == first.as_slice())
    );
    // Blue above grey and red below it: more blue than green than red.
    assert!(first[2] > first[1] && first[1] > first[0], "{first:?}");

    // Half as wide only (4:2:2).
    let picture = yuv(
        Sampling::Wide,
        true,
        BT601,
        Plane::new(&luma, 3, 3, 3, false).unwrap(),
        Some((
            Plane::new(&blue_difference, 2, 2, 2, false).unwrap(),
            Plane::new(&red_difference, 2, 2, 2, false).unwrap(),
        )),
    );
    assert_eq!(picture.to_rgba().rgba.len(), 3 * 3 * 4);
}

#[test]
fn a_chroma_edge_is_blended_between_its_two_samples() {
    // Four pixels in a row, chroma samples: grey, then reddish.
    let luma = [128u8; 4];
    let blue_difference = [128u8; 2];
    let red_difference = [128u8, 192];
    let picture = yuv(
        Sampling::Half,
        true,
        BT601,
        Plane::new(&luma, 4, 4, 1, false).unwrap(),
        Some((
            Plane::new(&blue_difference, 2, 2, 1, false).unwrap(),
            Plane::new(&red_difference, 2, 2, 1, false).unwrap(),
        )),
    );
    let out = picture.to_rgba();
    let red: Vec<u8> = out
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[0])
        .collect();
    assert!(red.windows(2).all(|pair| pair[0] <= pair[1]), "{red:?}");
    assert!(red[0] < red[3]);
    // The pixels between the samples are between the colours, not either.
    assert!(red[1] > red[0] && red[2] < red[3], "{red:?}");
}

#[test]
fn ten_bit_samples_are_read_as_pairs_of_bytes_and_scaled() {
    // Video range at 10 bits: black 64, white 940, centre 512.
    let (data, width, height) = wide(&[64, 502, 940], 3);
    let grey = wide(&[512, 512, 512], 3).0;
    let picture = yuv(
        Sampling::Full,
        false,
        BT709,
        Plane::new(&data, width * 2, width, height, true).unwrap(),
        Some((
            Plane::new(&grey, 6, 3, 1, true).unwrap(),
            Plane::new(&grey, 6, 3, 1, true).unwrap(),
        )),
    );
    let out = picture.to_rgba();
    near(&out.rgba[0..3], [0, 0, 0], 1);
    near(&out.rgba[4..7], [127, 127, 127], 2);
    near(&out.rgba[8..11], [255, 255, 255], 1);
}

#[test]
fn the_identity_matrix_holds_green_blue_and_red_planes() {
    // Y is green, U is blue, V is red, at full range.
    let green = [200u8];
    let blue = [50u8];
    let red = [10u8];
    let picture = yuv(
        Sampling::Full,
        true,
        None,
        bytes(&green, 1),
        Some((bytes(&blue, 1), bytes(&red, 1))),
    );
    near(&picture.to_rgba().rgba[0..3], [10, 200, 50], 1);
}

#[test]
fn a_monochrome_picture_is_grey_and_a_plane_can_be_read_as_opacity() {
    let luma = [0u8, 128, 255];
    let picture = yuv(Sampling::None, true, BT601, bytes(&luma, 3), None);
    let out = picture.to_rgba();
    near(&out.rgba[4..7], [128, 128, 128], 1);
    assert_eq!(picture.opacity(), [0, 128, 255]);
    // The same plane at video range: 16 is clear, 235 is solid.
    let limited = yuv(Sampling::None, false, BT601, bytes(&[16, 235], 2), None);
    assert_eq!(limited.opacity(), [0, 255]);
}

#[test]
fn alpha_is_written_and_taken_out_of_the_colours_when_they_were_multiplied() {
    let mut pixels = Pixels {
        width: 2,
        height: 1,
        rgba: vec![100, 50, 0, 255, 100, 50, 0, 255],
    };
    apply_alpha(&mut pixels, &[255, 128], false);
    assert_eq!(pixels.rgba, [100, 50, 0, 255, 100, 50, 0, 128]);
    // 100 is what 200 comes to at half opacity.
    apply_alpha(&mut pixels, &[255, 128], true);
    assert_eq!(pixels.rgba[4..8], [199, 99, 0, 128]);
    // Nothing is divided by a clear pixel.
    let mut clear = Pixels {
        width: 1,
        height: 1,
        rgba: vec![9, 9, 9, 255],
    };
    apply_alpha(&mut clear, &[0], true);
    assert_eq!(clear.rgba, [9, 9, 9, 0]);
}

#[test]
fn a_plane_shorter_than_its_picture_is_refused_rather_than_indexed() {
    assert!(Plane::new(&[0; 5], 3, 3, 2, false).is_err());
    assert!(
        Plane::new(&[0; 6], 2, 3, 2, false).is_err(),
        "stride < width"
    );
    assert!(Plane::new(&[0; 6], 3, 3, 2, false).is_ok());
    assert!(
        Plane::new(&[0; 6], 3, 3, 2, true).is_err(),
        "pairs of bytes"
    );
    assert!(Plane::new(&[], 0, 0, 0, false).is_err());
}

/// A 32 by 32 file libaom wrote through ffmpeg (4:2:0, video range, BT.709):
/// red on its left half, blue on its right half.
const HALVES: [u8; 331] = [
    0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66, 0x00, 0x00, 0x00, 0x00,
    0x61, 0x76, 0x69, 0x66, 0x6d, 0x69, 0x66, 0x31, 0x6d, 0x69, 0x61, 0x66, 0x4d, 0x41, 0x31, 0x42,
    0x00, 0x00, 0x00, 0xf9, 0x6d, 0x65, 0x74, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2f,
    0x68, 0x64, 0x6c, 0x72, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x70, 0x69, 0x63, 0x74,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x50, 0x69, 0x63, 0x74,
    0x75, 0x72, 0x65, 0x48, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x72, 0x00, 0x00, 0x00, 0x00, 0x0e, 0x70,
    0x69, 0x74, 0x6d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1e, 0x69, 0x6c, 0x6f,
    0x63, 0x00, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x01, 0x21, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x28, 0x69, 0x69, 0x6e, 0x66, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x1a, 0x69, 0x6e, 0x66, 0x65, 0x02, 0x00, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x61, 0x76, 0x30, 0x31, 0x43, 0x6f, 0x6c, 0x6f, 0x72, 0x00, 0x00,
    0x00, 0x00, 0x6a, 0x69, 0x70, 0x72, 0x70, 0x00, 0x00, 0x00, 0x4b, 0x69, 0x70, 0x63, 0x6f, 0x00,
    0x00, 0x00, 0x14, 0x69, 0x73, 0x70, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00,
    0x00, 0x00, 0x20, 0x00, 0x00, 0x00, 0x10, 0x70, 0x69, 0x78, 0x69, 0x00, 0x00, 0x00, 0x00, 0x03,
    0x08, 0x08, 0x08, 0x00, 0x00, 0x00, 0x0c, 0x61, 0x76, 0x31, 0x43, 0x81, 0x00, 0x0c, 0x00, 0x00,
    0x00, 0x00, 0x13, 0x63, 0x6f, 0x6c, 0x72, 0x6e, 0x63, 0x6c, 0x78, 0x00, 0x02, 0x00, 0x02, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x00, 0x17, 0x69, 0x70, 0x6d, 0x61, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x01, 0x00, 0x01, 0x04, 0x01, 0x02, 0x83, 0x04, 0x00, 0x00, 0x00, 0x32, 0x6d, 0x64, 0x61,
    0x74, 0x0a, 0x09, 0x18, 0x11, 0x3f, 0xf6, 0x88, 0x10, 0x10, 0x08, 0x20, 0x32, 0x1d, 0x15, 0x00,
    0x00, 0x00, 0x48, 0x00, 0x10, 0xb4, 0xce, 0x60, 0x8a, 0xe1, 0xa4, 0x74, 0x63, 0xff, 0xa9, 0x08,
    0xde, 0xb5, 0x19, 0x67, 0xde, 0xcb, 0xad, 0x4a, 0x1e, 0x5e, 0x37,
];

/// A picture the encoder in the `image` crate wrote, to read back.
fn encoded(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    use ::image::codecs::avif::AvifEncoder;
    use ::image::{ExtendedColorType, ImageEncoder};
    let mut rgba = Vec::new();
    for y in 0..height {
        for x in 0..width {
            rgba.extend(pixel(x, y));
        }
    }
    let mut out = Vec::new();
    AvifEncoder::new_with_speed_quality(&mut out, 10, 100)
        .write_image(&rgba, width, height, ExtendedColorType::Rgba8)
        .expect("encoded");
    out
}

#[test]
fn a_real_file_with_an_odd_size_and_an_alpha_plane_reads_back() {
    // 33 by 17, red on the left and blue on the right; transparent on the
    // bottom row.
    let file = encoded(33, 17, |x, y| {
        let alpha = if y == 16 { 0 } else { 255 };
        if x < 16 {
            [220, 20, 20, alpha]
        } else {
            [20, 20, 220, alpha]
        }
    });
    assert!(is_avif(&file[..32.min(file.len())]));
    let out = decode(&file).expect("decoded");
    assert_eq!((out.width, out.height), (33, 17));
    assert_eq!(out.rgba.len(), 33 * 17 * 4);
    let at = |x: usize, y: usize| &out.rgba[(y * 33 + x) * 4..(y * 33 + x) * 4 + 4];
    near(&at(3, 3)[..3], [220, 20, 20], 24);
    near(&at(29, 3)[..3], [20, 20, 220], 24);
    assert!(at(3, 3)[3] > 240, "{:?}", at(3, 3));
    assert!(at(3, 16)[3] < 16, "{:?}", at(3, 16));
}

#[test]
fn a_file_cut_short_or_not_an_image_is_an_error_not_a_panic() {
    let file = encoded(16, 16, |_, _| [10, 200, 10, 255]);
    assert!(decode(&file).is_ok());
    for cut in [0, 8, 20, file.len() / 2, file.len() - 3] {
        // The result does not matter, only that it ends.
        let _ = decode(&file[..cut]);
    }
    assert!(decode(b"not an image at all").is_err());
    assert!(decode(&[]).is_err());
}

#[test]
fn a_file_from_another_encoder_in_video_range_with_half_the_chroma_reads_back() {
    let out = decode(&HALVES).expect("decoded");
    assert_eq!((out.width, out.height), (32, 32));
    let at = |x: usize, y: usize| &out.rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
    // Away from the edge between the halves, where the chroma is blended.
    for y in [2, 16, 29] {
        near(&at(4, y)[..3], [200, 30, 30], 14);
        near(&at(27, y)[..3], [30, 30, 200], 14);
        assert_eq!(at(4, y)[3], 255);
    }
}
