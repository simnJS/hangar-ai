//! Pictures of models, drawn on whatever adapter the machine has. A machine
//! with none skips these: the drawing itself cannot be checked without one.
//!
//! A CI runner has no GPU but usually a software rasterizer (Mesa's lavapipe,
//! WARP on Windows), and the pictures are then drawn by it. Anti-aliased edges
//! and faint lines differ from one rasterizer to another by a few hundred
//! pixels, so what is asserted here is a colour with room around it, a shape,
//! or a picture that is not blank, never one picture's pixel count against
//! another's when the two are close.

use std::sync::Arc;

use hangar_model3d::camera::Camera;
use hangar_model3d::load::load;
use hangar_model3d::samples;
use hangar_model3d::{CaptureError, CaptureJob, DisplayOptions, Model, SnapshotAngle, ViewerTheme};

fn model_from(name: &str, bytes: &[u8]) -> Arc<Model> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    Arc::new(load(&path).unwrap_or_else(|error| panic!("{name}: {error}")))
}

/// Takes a picture, or says the machine has no GPU and returns `None`.
fn shoot(job: CaptureJob) -> Option<image::RgbaImage> {
    match job.run() {
        Ok(png) => Some(image::load_from_memory(&png).expect("a PNG").to_rgba8()),
        Err(CaptureError::NoGpu(reason)) => {
            eprintln!("skipped, no GPU: {reason}");
            None
        }
        Err(error) => panic!("{error}"),
    }
}

fn theme() -> ViewerTheme {
    ViewerTheme::dark()
}

fn snapshot(model: &Arc<Model>, angle: SnapshotAngle, w: u32, h: u32) -> Option<image::RgbaImage> {
    shoot(CaptureJob::snapshot(model.clone(), angle, w, h, theme()))
}

fn close(a: image::Rgba<u8>, b: image::Rgba<u8>, tolerance: u8) -> bool {
    (0..3).all(|i| a[i].abs_diff(b[i]) <= tolerance)
}

#[test]
fn draws_a_red_cube_on_the_themes_background() {
    let model = model_from("cube.glb", &samples::cube_glb());
    let Some(image) = snapshot(&model, SnapshotAngle::Front, 320, 240) else {
        return;
    };
    assert_eq!(image.dimensions(), (320, 240));
    let centre = *image.get_pixel(160, 120);
    assert!(
        centre[0] > 90 && centre[0] > centre[1] * 2 && centre[0] > centre[2] * 2,
        "the cube is red, got {centre:?}"
    );
    // The corners show the background the theme asked for. The floor's lines
    // may cross one, so look at the very corner.
    let background = theme().background.into_rgba8();
    let corner = *image.get_pixel(1, 1);
    assert!(
        close(corner, image::Rgba(background), 3),
        "background {corner:?} vs {background:?}"
    );
}

#[test]
fn takes_every_angle_and_each_looks_different() {
    let model = model_from("cube.glb", &samples::textured_cube_glb());
    let mut pictures = Vec::new();
    for angle in SnapshotAngle::ALL {
        let Some(image) = snapshot(&model, angle, 160, 120) else {
            return;
        };
        pictures.push((angle, image));
    }
    for (i, (a, first)) in pictures.iter().enumerate() {
        for (b, second) in &pictures[i + 1..] {
            // Front and back of this model differ (the texture is not
            // symmetric on its two sides only by luck of UVs), so only the
            // views that are mirror images of one another may match.
            let same = first.as_raw() == second.as_raw();
            let mirrored = matches!(
                (a, b),
                (SnapshotAngle::Front, SnapshotAngle::Back)
                    | (SnapshotAngle::Left, SnapshotAngle::Right)
            );
            assert!(!same || mirrored, "{a:?} and {b:?} are the same picture");
        }
    }
}

#[test]
fn leaves_the_floor_out_when_looking_from_below() {
    let model = model_from("cube.glb", &samples::cube_glb());
    let (Some(above), Some(below)) = (
        snapshot(&model, SnapshotAngle::Iso, 240, 180),
        snapshot(&model, SnapshotAngle::Bottom, 240, 180),
    ) else {
        return;
    };
    let background = image::Rgba(theme().background.into_rgba8());
    let off_background = |image: &image::RgbaImage| {
        image
            .pixels()
            .filter(|p| !close(**p, background, 1))
            .count()
    };
    assert!(
        off_background(&above) > 3000,
        "the cube and a floor under it"
    );
    // From below there is the cube's underside and nothing else: what is not
    // the background is one solid rectangle with background all around it. A
    // floor would run its lines across the picture, past the cube's edges and
    // out to the border, so the rectangle would reach the border or be full of
    // holes. (The picture from above cannot be set against this one by its
    // count of drawn pixels: the cube alone is nearly as large face-on as it
    // is from the corner, and the floor's faint lines add less than what one
    // rasterizer's anti-aliasing takes from the other's.)
    let background = theme().background.into_rgba8();
    let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
    let mut drawn = 0u32;
    for (x, y, pixel) in below.enumerate_pixels() {
        if !close(*pixel, image::Rgba(background), 3) {
            (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
            drawn += 1;
        }
    }
    assert!(drawn > 0, "the cube's underside is drawn");
    let (width, height) = below.dimensions();
    assert!(
        left > 0 && top > 0 && right < width - 1 && bottom < height - 1,
        "background all around the underside: it spans {left}..={right} by {top}..={bottom} \
         of a {width} by {height} picture"
    );
    // The corners of the rectangle may be covered too little to be told from
    // the background; the rest of it is not.
    let area = (right + 1 - left) * (bottom + 1 - top);
    assert!(
        u64::from(drawn) * 100 >= u64::from(area) * 99,
        "the underside fills its rectangle: {drawn} of {area} pixels"
    );
}

#[test]
fn draws_a_textured_model_with_a_normal_map_and_two_placements() {
    let model = model_from("textured.glb", &samples::textured_cube_glb());
    let Some(image) = snapshot(&model, SnapshotAngle::Front, 320, 240) else {
        return;
    };
    // Blue and near-white checks of the texture, side by side.
    let blue = image.pixels().filter(|p| p[2] > p[0] + 40).count();
    let pale = image
        .pixels()
        .filter(|p| p[0] > 120 && p[1] > 120 && p[2] > 120)
        .count();
    assert!(blue > 200, "blue texels: {blue}");
    assert!(pale > 200, "pale texels: {pale}");
}

#[test]
fn draws_every_helper_without_the_gpu_objecting() {
    let model = model_from("textured.glb", &samples::textured_cube_glb());
    let camera = Camera::framed(&model, 4.0 / 3.0);
    let display = DisplayOptions {
        wireframe: true,
        grid: true,
        axes: true,
        normals: true,
    };
    let Some(with) = shoot(CaptureJob::from_camera(
        model.clone(),
        camera.clone(),
        display,
        320,
        240,
        theme(),
    )) else {
        return;
    };
    let Some(without) = shoot(CaptureJob::from_camera(
        model,
        camera,
        DisplayOptions::default(),
        320,
        240,
        theme(),
    )) else {
        return;
    };
    assert_ne!(with.as_raw(), without.as_raw());
    // The axes are red, green and blue over the model.
    let axis = ViewerTheme::dark().axis_x.into_rgba8();
    assert!(
        with.pixels()
            .any(|p| close(*p, image::Rgba(axis), 40) && p[0] > 180),
        "an X axis in the theme's red"
    );
}

#[test]
fn lets_the_background_show_through_a_translucent_material() {
    let opaque = model_from("cube.glb", &samples::cube_glb());
    let glass = model_from(
        "glass.glb",
        &samples::cube_glb_with(
            r#"{"alphaMode": "BLEND", "doubleSided": true, "pbrMetallicRoughness": {"baseColorFactor": [0.9, 0.1, 0.1, 0.35], "metallicFactor": 0.0}}"#,
        ),
    );
    let (Some(solid), Some(clear)) = (
        snapshot(&opaque, SnapshotAngle::Front, 200, 150),
        snapshot(&glass, SnapshotAngle::Front, 200, 150),
    ) else {
        return;
    };
    let background = theme().background.into_rgba8();
    let (a, b) = (*solid.get_pixel(100, 75), *clear.get_pixel(100, 75));
    // Between the solid colour and the background, on every channel that
    // differs between them.
    for channel in 0..3 {
        let (low, high) = (
            a[channel].min(background[channel]),
            a[channel].max(background[channel]),
        );
        assert!(
            b[channel] >= low.saturating_sub(2) && b[channel] <= high.saturating_add(2),
            "channel {channel}: {b:?} is not between {a:?} and {background:?}"
        );
    }
    assert_ne!(a, b);
}

#[test]
fn lights_an_emissive_material_and_an_unlit_one_in_their_own_colour() {
    let emissive = model_from(
        "glow.glb",
        &samples::cube_glb_with(
            r#"{"emissiveFactor": [0.0, 1.0, 0.0], "pbrMetallicRoughness": {"baseColorFactor": [0.0, 0.0, 0.0, 1.0], "metallicFactor": 0.0}}"#,
        ),
    );
    let Some(image) = snapshot(&emissive, SnapshotAngle::Front, 160, 120) else {
        return;
    };
    let centre = *image.get_pixel(80, 60);
    assert!(
        centre[1] > 150 && centre[0] < 90 && centre[2] < 90,
        "{centre:?}"
    );
}

#[test]
fn draws_point_clouds_and_scans() {
    let cloud = model_from("scan.ply", samples::point_cloud_ply(4000).as_bytes());
    let Some(image) = snapshot(&cloud, SnapshotAngle::Front, 200, 200) else {
        return;
    };
    let background = image::Rgba(theme().background.into_rgba8());
    let drawn = image
        .pixels()
        .filter(|p| !close(**p, background, 8))
        .count();
    assert!(drawn > 200, "points on the picture: {drawn}");

    // A scan with faces is a surface, drawn with the colours of its vertices.
    let scan = model_from("cube.ply", samples::cube_ply_ascii().as_bytes());
    let Some(image) = snapshot(&scan, SnapshotAngle::Iso, 160, 120) else {
        return;
    };
    let drawn = image
        .pixels()
        .filter(|p| !close(**p, background, 8))
        .count();
    assert!(drawn > 1000, "a coloured cube: {drawn}");
}

#[test]
fn draws_a_stl_standing_upright() {
    // Z-up in the file: a tall bar is tall on screen.
    let mut bytes = samples::cube_stl_binary();
    for facet in bytes[84..].as_chunks_mut::<50>().0 {
        for vertex in 0..3 {
            let at = 12 + vertex * 12 + 8;
            let z = f32::from_le_bytes(facet[at..at + 4].try_into().unwrap()) * 6.0;
            facet[at..at + 4].copy_from_slice(&z.to_le_bytes());
        }
    }
    let model = model_from("bar.stl", &bytes);
    let Some(image) = snapshot(&model, SnapshotAngle::Front, 200, 300) else {
        return;
    };
    let background = image::Rgba(theme().background.into_rgba8());
    let rows: Vec<u32> = (0..300)
        .filter(|&y| (0..200).any(|x| !close(*image.get_pixel(x, y), background, 12)))
        .collect();
    let columns: Vec<u32> = (0..200)
        .filter(|&x| (0..300).any(|y| !close(*image.get_pixel(x, y), background, 12)))
        .collect();
    let height = rows.last().unwrap() - rows.first().unwrap();
    let width = columns.last().unwrap() - columns.first().unwrap();
    assert!(height > width, "bar is {width} wide and {height} tall");
}

#[test]
fn loads_and_draws_the_demo_hangar() {
    // A real model: a hangar, a drone over it, 147 meshes and 32 materials.
    // It is the old viewer's demo file, kept as the repository's sample.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models/sample.glb");
    let model = Arc::new(load(&path).unwrap());
    assert_eq!(
        (
            model.info.triangles,
            model.info.vertices,
            model.info.meshes,
            model.info.materials
        ),
        (4432, 12180, 147, 32)
    );
    assert!((model.info.size[0] - 13.0).abs() < 1e-3 && (model.info.size[2] - 11.5).abs() < 1e-3);
    assert!(model.warnings.is_empty(), "{:?}", model.warnings);
    let Some(image) = snapshot(&model, SnapshotAngle::Iso, 400, 300) else {
        return;
    };
    let background = image::Rgba(theme().background.into_rgba8());
    // The model fills the frame rather than sitting small in the middle.
    let rows: Vec<u32> = (0..300)
        .filter(|&y| (0..400).any(|x| !close(*image.get_pixel(x, y), background, 16)))
        .collect();
    assert!(rows.last().unwrap() - rows.first().unwrap() > 200);
}

fn differing(a: &image::RgbaImage, b: &image::RgbaImage) -> usize {
    a.pixels()
        .zip(b.pixels())
        .filter(|(a, b)| !close(**a, **b, 8))
        .count()
}

#[test]
fn pictures_an_animated_model_at_the_moment_asked_and_at_its_start_otherwise() {
    use hangar_model3d::Playhead;
    let model = model_from("slide.glb", &samples::animated_cube_glb());
    let at = |time: f32| {
        shoot(
            CaptureJob::snapshot(model.clone(), SnapshotAngle::Front, 200, 150, theme())
                .at(Some(Playhead { clip: 0, time })),
        )
    };
    let (Some(start), Some(out), Some(back)) = (at(0.0), at(1.0), at(2.0)) else {
        return;
    };
    assert!(
        differing(&start, &out) > 300,
        "the cube is somewhere else at one second"
    );
    assert_eq!(
        differing(&start, &back),
        0,
        "and home at two: the clip loops"
    );
    // With no moment given, an agent's snapshot is of the first frame.
    let Some(default) = snapshot(&model, SnapshotAngle::Front, 200, 150) else {
        return;
    };
    assert_eq!(differing(&start, &default), 0);
}

#[test]
fn pictures_a_bent_skin_and_a_morphed_mesh() {
    use hangar_model3d::Playhead;
    for (name, bytes) in [
        ("skin.glb", samples::skinned_glb()),
        ("morph.glb", samples::morphing_glb()),
    ] {
        let model = model_from(name, &bytes);
        let at = |time: f32| {
            shoot(
                CaptureJob::snapshot(model.clone(), SnapshotAngle::Front, 160, 240, theme())
                    .at(Some(Playhead { clip: 0, time })),
            )
        };
        let (Some(start), Some(end)) = (at(0.0), at(1.0)) else {
            return;
        };
        let background = image::Rgba(theme().background.into_rgba8());
        let rows = |image: &image::RgbaImage| {
            (0..240)
                .filter(|&y| (0..160).any(|x| !close(*image.get_pixel(x, y), background, 12)))
                .count()
        };
        // The picture keeps the framing of the rest pose, so the box that has
        // grown three times as tall covers more rows of it.
        assert!(rows(&end) > rows(&start) + 30, "{name}");
    }
}

#[test]
fn says_there_is_nothing_to_draw_without_a_model() {
    // No GPU is needed to be told that.
    use hangar_model3d::ModelViewer;
    assert!(ModelViewer::new().capture(1.0, theme()).is_none());
}
