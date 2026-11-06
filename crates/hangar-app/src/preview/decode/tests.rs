use super::*;

use ::image::codecs::gif::{GifEncoder, Repeat};
use ::image::{Delay, Frame as ImageFrame, Rgba, RgbaImage};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A GIF of solid frames, each shown for its own delay.
fn gif(size: (u32, u32), frames: &[([u8; 4], u32)]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new(&mut out);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for (colour, delay) in frames {
            let image = RgbaImage::from_pixel(size.0, size.1, Rgba(*colour));
            let frame = ImageFrame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(*delay, 1));
            encoder.encode_frame(frame).unwrap();
        }
    }
    out
}

/// A WebP of three 6 by 4 frames, red, green and blue, shown for 100, 200 and
/// 300 ms, looping forever (written by libwebp's `img2webp` through Pillow).
const ANIMATED_WEBP: [u8; 188] = [
    0x52, 0x49, 0x46, 0x46, 0xb4, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50, 0x38, 0x58,
    0x0a, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x03, 0x00, 0x00, 0x41, 0x4e,
    0x49, 0x4d, 0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x41, 0x4e, 0x4d, 0x46,
    0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x03, 0x00, 0x00,
    0x64, 0x00, 0x00, 0x02, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x05, 0xc0, 0x00,
    0x00, 0x07, 0x10, 0xfd, 0x8f, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46,
    0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x03, 0x00, 0x00,
    0xc8, 0x00, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x05, 0xc0, 0x00,
    0x00, 0x07, 0xd0, 0xff, 0x88, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00, 0x41, 0x4e, 0x4d, 0x46,
    0x28, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x03, 0x00, 0x00,
    0x2c, 0x01, 0x00, 0x00, 0x56, 0x50, 0x38, 0x4c, 0x0f, 0x00, 0x00, 0x00, 0x2f, 0x05, 0xc0, 0x00,
    0x00, 0x07, 0x10, 0xd1, 0xff, 0xfe, 0x07, 0x22, 0xa2, 0xff, 0x01, 0x00,
];

/// Whether two pixels are the same colour, give or take one step of a channel:
/// the WebP decoder blends each frame onto the canvas, and does not always
/// land on the exact 255 it was given.
fn same(got: [u8; 4], want: [u8; 4]) -> bool {
    got.iter()
        .zip(want)
        .all(|(got, want)| got.abs_diff(want) <= 1)
}

/// The first pixel of a frame.
fn first_pixel(frame: &Frame) -> [u8; 4] {
    let iced::widget::image::Handle::Rgba { pixels, .. } = &frame.handle else {
        panic!("a frame is pixels");
    };
    [pixels[0], pixels[1], pixels[2], pixels[3]]
}

fn animated(loaded: Result<Loaded, String>) -> Arc<Animation> {
    match loaded.expect("loaded") {
        Loaded::Animated(animation) => animation,
        other => panic!("not animated: {other:?}"),
    }
}

#[test]
fn a_timeline_tells_the_frame_showing_and_what_is_left_of_it() {
    let timeline = Timeline::new([ms(100), ms(200), ms(300)]);
    assert_eq!(timeline.total(), ms(600));
    assert_eq!(timeline.at(ms(0)), (0, ms(100)));
    assert_eq!(timeline.at(ms(99)), (0, ms(1)));
    assert_eq!(timeline.at(ms(100)), (1, ms(200)));
    assert_eq!(timeline.at(ms(299)), (1, ms(1)));
    assert_eq!(timeline.at(ms(300)), (2, ms(300)));
    assert_eq!(timeline.at(ms(599)), (2, ms(1)));
}

#[test]
fn a_timeline_loops_for_as_long_as_it_is_looked_at() {
    let timeline = Timeline::new([ms(100), ms(200), ms(300)]);
    assert_eq!(timeline.at(ms(600)), (0, ms(100)));
    assert_eq!(timeline.at(ms(750)), (1, ms(150)));
    assert_eq!(timeline.at(ms(600 * 1000 + 350)), (2, ms(250)));
    // A day on the clock is no harder than a second.
    assert_eq!(timeline.at(Duration::from_secs(86_400)).0, 0);
}

#[test]
fn a_frame_that_asks_for_no_time_is_shown_for_a_tenth_of_a_second() {
    // Encoders write 0 or 1 hundredth for "as fast as you can", which
    // browsers play at 10 frames a second; an exact 20 ms is kept.
    let timeline = Timeline::new([ms(0), ms(10), ms(19), ms(20)]);
    assert_eq!(timeline.total(), ms(100 * 3 + 20));
    assert_eq!(timeline.at(ms(150)), (1, ms(50)));
    assert_eq!(timeline.at(ms(300)).0, 3);
}

#[test]
fn a_timeline_without_frames_does_not_divide_by_nothing() {
    let timeline = Timeline::new([]);
    assert_eq!(timeline.total(), Duration::ZERO);
    assert_eq!(timeline.at(ms(500)).0, 0);
}

#[test]
fn a_gif_of_several_frames_is_played_with_the_time_of_each() {
    let file = gif(
        (8, 6),
        &[
            ([255, 0, 0, 255], 100),
            ([0, 255, 0, 255], 250),
            ([0, 0, 255, 255], 40),
        ],
    );
    let animation = animated(load(file));
    assert_eq!((animation.width, animation.height), (8, 6));
    assert_eq!(animation.frames.len(), 3);
    let delays: Vec<_> = animation.frames.iter().map(|f| f.delay).collect();
    assert_eq!(delays, [ms(100), ms(250), ms(40)]);
    assert_eq!(first_pixel(&animation.frames[0]), [255, 0, 0, 255]);
    assert_eq!(first_pixel(&animation.frames[1]), [0, 255, 0, 255]);
    assert_eq!(first_pixel(&animation.frames[2]), [0, 0, 255, 255]);
    assert_eq!(animation.timeline.total(), ms(390));
    // Each frame covers the canvas: 8 by 6 pixels of four bytes.
    let iced::widget::image::Handle::Rgba { pixels, .. } = &animation.frames[1].handle else {
        panic!("pixels");
    };
    assert_eq!(pixels.len(), 8 * 6 * 4);
}

/// The pixels of a still picture, and its size.
fn pixels(loaded: Result<Loaded, String>) -> (Vec<u8>, u32, u32) {
    match loaded.expect("loaded") {
        Loaded::Pixels(Handle::Rgba { pixels, .. }, width, height) => {
            (pixels.to_vec(), width, height)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_gif_of_one_frame_is_decoded_to_pixels() {
    let file = gif((4, 4), &[([1, 2, 3, 255], 100)]);
    let (rgba, width, height) = pixels(load(file));
    assert_eq!((width, height), (4, 4));
    assert!(same([rgba[0], rgba[1], rgba[2], rgba[3]], [1, 2, 3, 255]));
}

#[test]
fn a_png_is_decoded_to_pixels_off_the_ui_thread() {
    let image = RgbaImage::from_pixel(5, 3, Rgba([10, 20, 30, 255]));
    let mut file = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut file), ::image::ImageFormat::Png)
        .unwrap();
    let (rgba, width, height) = pixels(load(file));
    assert_eq!((width, height), (5, 3));
    assert_eq!(rgba.len(), 5 * 3 * 4);
    assert_eq!(&rgba[..4], &[10, 20, 30, 255]);
}

#[test]
fn a_picture_that_claims_too_many_pixels_is_refused_before_it_is_decoded() {
    // A BMP header that says 20000 × 20000 (400 million pixels), and nothing
    // after it: refused on its size, not on its missing data.
    let mut file = Vec::new();
    file.extend_from_slice(b"BM");
    file.extend_from_slice(&0u32.to_le_bytes()); // file size, unchecked
    file.extend_from_slice(&0u32.to_le_bytes()); // reserved
    file.extend_from_slice(&54u32.to_le_bytes()); // pixels start
    file.extend_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
    file.extend_from_slice(&20_000i32.to_le_bytes());
    file.extend_from_slice(&20_000i32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes()); // planes
    file.extend_from_slice(&24u16.to_le_bytes()); // bits a pixel
    file.extend_from_slice(&[0; 24]); // no compression, sizes, palette
    let why = load(file).unwrap_err();
    assert!(why.contains("20000 × 20000"), "{why}");
}

#[test]
fn a_gif_is_told_by_its_bytes_and_not_by_its_name() {
    // `load` is given nothing but bytes, so the name cannot matter.
    let file = gif((4, 4), &[([1, 2, 3, 255], 100), ([4, 5, 6, 255], 100)]);
    assert_eq!(animated(load(file)).frames.len(), 2);
}

#[test]
fn an_animated_webp_is_played_with_the_time_of_each_frame() {
    let animation = animated(load(ANIMATED_WEBP.to_vec()));
    assert_eq!((animation.width, animation.height), (6, 4));
    let delays: Vec<_> = animation.frames.iter().map(|f| f.delay).collect();
    assert_eq!(delays, [ms(100), ms(200), ms(300)]);
    assert!(same(first_pixel(&animation.frames[0]), [255, 0, 0, 255]));
    assert!(same(first_pixel(&animation.frames[1]), [0, 255, 0, 255]));
    assert!(same(first_pixel(&animation.frames[2]), [0, 0, 255, 255]));
}

#[test]
fn a_file_that_is_not_a_readable_picture_fails() {
    // A PNG cut after its signature, text, nothing, a GIF cut off in its
    // header, and a WebP of junk.
    for bytes in [
        b"\x89PNG\r\n\x1a\n".to_vec(),
        b"hello".to_vec(),
        Vec::new(),
        b"GIF89a".to_vec(),
        b"RIFF\0\0\0\0WEBPjunk".to_vec(),
    ] {
        assert!(load(bytes.clone()).is_err(), "{bytes:?}");
    }
}

#[test]
fn a_gif_broken_after_some_frames_plays_the_frames_it_has() {
    let file = gif(
        (8, 6),
        &[
            ([255, 0, 0, 255], 100),
            ([0, 255, 0, 255], 100),
            ([0, 0, 255, 255], 100),
        ],
    );
    // Cut in the last frame's data: the first two are what a browser plays.
    let cut = file[..file.len() - 12].to_vec();
    let animation = animated(load(cut));
    assert!(animation.frames.len() >= 2, "{}", animation.frames.len());
    assert_eq!(first_pixel(&animation.frames[0]), [255, 0, 0, 255]);
}

#[test]
fn two_loads_of_one_file_are_two_animations() {
    let file = gif((4, 4), &[([1, 2, 3, 255], 100), ([4, 5, 6, 255], 100)]);
    let (a, b) = (animated(load(file.clone())), animated(load(file)));
    assert_eq!(*a, *a);
    assert_ne!(*a, *b);
}

#[test]
fn an_avif_is_decoded_to_pixels() {
    use ::image::codecs::avif::AvifEncoder;
    use ::image::{ExtendedColorType, ImageEncoder};
    let rgba = RgbaImage::from_pixel(10, 7, Rgba([20, 220, 20, 255])).into_raw();
    let mut file = Vec::new();
    AvifEncoder::new_with_speed_quality(&mut file, 10, 100)
        .write_image(&rgba, 10, 7, ExtendedColorType::Rgba8)
        .unwrap();
    match load(file).expect("loaded") {
        Loaded::Pixels(
            Handle::Rgba {
                width,
                height,
                pixels,
                ..
            },
            w,
            h,
        ) => {
            assert_eq!((width, height, w, h), (10, 7, 10, 7));
            assert_eq!(pixels.len(), 10 * 7 * 4);
            assert!(pixels[1] > 180 && pixels[0] < 60, "{:?}", &pixels[..4]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_over_the_cap_is_not_read() {
    let dir = crate::kit_tabs::testdir::scratch("decode");
    let small = dir.join("small.bin");
    std::fs::write(&small, b"abc").unwrap();
    assert_eq!(read_file(&small).unwrap(), b"abc");
    assert!(read_file(&dir.join("missing.png")).is_err());
    // A sparse file as large as the cap plus one byte, which costs no disk.
    let big = dir.join("big.bin");
    let file = std::fs::File::create(&big).unwrap();
    file.set_len(MAX_BYTES + 1).unwrap();
    let why = read_file(&big).unwrap_err();
    assert!(why.contains("larger than the 64 MB"), "{why}");
    let _ = std::fs::remove_dir_all(&dir);
}
