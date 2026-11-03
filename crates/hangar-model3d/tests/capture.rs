//! Pictures of models, drawn on whatever GPU the machine has. A machine with
//! none (a CI runner) skips these: the drawing itself cannot be checked
//! without one.

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
    // From below there is the cube's underside and nothing else.
    assert!(off_background(&below) < off_background(&above));
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
fn says_there_is_nothing_to_draw_without_a_model() {
    // No GPU is needed to be told that.
    use hangar_model3d::ModelViewer;
    assert!(ModelViewer::new().capture(1.0, theme()).is_none());
}
