//! Where the camera goes to show a whole model, as plain arithmetic on
//! arrays: no GPU and no matrix library, so the tests can check it exactly.
//!
//! Models are taken as glTF lays them out — +Y up, the front facing +Z —
//! which is also what every loader hands back. "right" is the camera on +X, as
//! in Blender's numpad 3.

use std::str::FromStr;

pub type Vec3 = [f64; 3];

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds {
        min: [0.0; 3],
        max: [0.0; 3],
    };

    pub fn size(&self) -> Vec3 {
        sub(self.max, self.min)
    }

    pub fn center(&self) -> Vec3 {
        center_of(self)
    }

    /// The eight corners, which is all a box has to fit.
    pub fn corners(&self) -> [Vec3; 8] {
        let (min, max) = (self.min, self.max);
        [
            [min[0], min[1], min[2]],
            [min[0], min[1], max[2]],
            [min[0], max[1], min[2]],
            [min[0], max[1], max[2]],
            [max[0], min[1], min[2]],
            [max[0], min[1], max[2]],
            [max[0], max[1], min[2]],
            [max[0], max[1], max[2]],
        ]
    }

    /// The smallest box around some points; the empty box for none.
    pub fn around(points: impl IntoIterator<Item = Vec3>) -> Bounds {
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        let mut any = false;
        for point in points {
            any = true;
            for axis in 0..3 {
                min[axis] = min[axis].min(point[axis]);
                max[axis] = max[axis].max(point[axis]);
            }
        }
        if any {
            Bounds { min, max }
        } else {
            Bounds::EMPTY
        }
    }
}

/// The views an agent can ask a picture of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotAngle {
    Front,
    Back,
    Left,
    Right,
    Top,
    Bottom,
    Iso,
}

impl SnapshotAngle {
    pub const ALL: [SnapshotAngle; 7] = [
        SnapshotAngle::Front,
        SnapshotAngle::Back,
        SnapshotAngle::Left,
        SnapshotAngle::Right,
        SnapshotAngle::Top,
        SnapshotAngle::Bottom,
        SnapshotAngle::Iso,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SnapshotAngle::Front => "front",
            SnapshotAngle::Back => "back",
            SnapshotAngle::Left => "left",
            SnapshotAngle::Right => "right",
            SnapshotAngle::Top => "top",
            SnapshotAngle::Bottom => "bottom",
            SnapshotAngle::Iso => "iso",
        }
    }
}

impl FromStr for SnapshotAngle {
    type Err = String;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        SnapshotAngle::ALL
            .into_iter()
            .find(|angle| angle.name() == name.trim().to_ascii_lowercase())
            .ok_or_else(|| {
                format!(
                    "Unknown angle \"{name}\": use one of {}.",
                    SnapshotAngle::ALL.map(SnapshotAngle::name).join(", ")
                )
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewDirection {
    /// Unit vector from the model's centre toward the camera.
    pub dir: Vec3,
    /// Which way is up on screen. Never parallel to `dir`.
    pub up: Vec3,
}

/// The three-quarter view every model opens on, and the "iso" snapshot: from
/// the front right, 30° above the horizon. Lower than a true isometric (35°),
/// which flattens a building into its roof.
pub const DEFAULT_AZIMUTH: f64 = std::f64::consts::FRAC_PI_4;
pub const DEFAULT_ELEVATION: f64 = std::f64::consts::FRAC_PI_6;

/// A direction from an azimuth (0 = front, positive toward +X) and an
/// elevation.
pub fn direction_from(azimuth: f64, elevation: f64) -> Vec3 {
    let flat = elevation.cos();
    [flat * azimuth.sin(), elevation.sin(), flat * azimuth.cos()]
}

/// The canonical views an agent can ask for. Looking straight down, the
/// model's front is at the bottom of the image; straight up, at the top — the
/// way a plan and a reflected ceiling plan are drawn.
pub fn view_direction(angle: SnapshotAngle) -> ViewDirection {
    match angle {
        SnapshotAngle::Front => ViewDirection {
            dir: [0.0, 0.0, 1.0],
            up: [0.0, 1.0, 0.0],
        },
        SnapshotAngle::Back => ViewDirection {
            dir: [0.0, 0.0, -1.0],
            up: [0.0, 1.0, 0.0],
        },
        SnapshotAngle::Left => ViewDirection {
            dir: [-1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
        },
        SnapshotAngle::Right => ViewDirection {
            dir: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
        },
        SnapshotAngle::Top => ViewDirection {
            dir: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, -1.0],
        },
        SnapshotAngle::Bottom => ViewDirection {
            dir: [0.0, -1.0, 0.0],
            up: [0.0, 0.0, 1.0],
        },
        SnapshotAngle::Iso => default_view(),
    }
}

/// The three-quarter view a model opens on.
pub fn default_view() -> ViewDirection {
    ViewDirection {
        dir: direction_from(DEFAULT_AZIMUTH, DEFAULT_ELEVATION),
        up: [0.0, 1.0, 0.0],
    }
}

pub(crate) fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn scale(a: Vec3, factor: f64) -> Vec3 {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

pub(crate) fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

pub fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l > 0.0 {
        [a[0] / l, a[1] / l, a[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}

pub fn center_of(bounds: &Bounds) -> Vec3 {
    [
        (bounds.min[0] + bounds.max[0]) / 2.0,
        (bounds.min[1] + bounds.max[1]) / 2.0,
        (bounds.min[2] + bounds.max[2]) / 2.0,
    ]
}

/// Half the box's diagonal: the radius of the sphere around it.
pub fn radius_of(bounds: &Bounds) -> f64 {
    length(sub(bounds.max, bounds.min)) / 2.0
}

/// What is framed: the model's box, or better, points on the model itself — a
/// sample of its vertices. A box frames its empty corners too: a drone flying
/// high over a building stretches the box up, and the building ends up low in
/// the frame under a box's worth of sky.
#[derive(Clone, Copy, Debug)]
pub enum Shape<'a> {
    Box(Bounds),
    Points(&'a [Vec3]),
}

impl From<Bounds> for Shape<'_> {
    fn from(bounds: Bounds) -> Self {
        Shape::Box(bounds)
    }
}

impl<'a> From<&'a [Vec3]> for Shape<'a> {
    fn from(points: &'a [Vec3]) -> Self {
        Shape::Points(points)
    }
}

impl<'a> From<&'a Vec<Vec3>> for Shape<'a> {
    fn from(points: &'a Vec<Vec3>) -> Self {
        Shape::Points(points)
    }
}

impl Shape<'_> {
    fn points(&self) -> Vec<Vec3> {
        match self {
            Shape::Box(bounds) => bounds.corners().to_vec(),
            Shape::Points(points) => points.to_vec(),
        }
    }

    fn bounds(&self) -> Bounds {
        match self {
            Shape::Box(bounds) => *bounds,
            Shape::Points(points) => Bounds::around(points.iter().copied()),
        }
    }
}

/// The camera's axes for a view: where it looks, its right, its up.
fn basis(view: &ViewDirection) -> (Vec3, Vec3, Vec3) {
    let dir = normalize(view.dir);
    let right = normalize(cross(view.up, dir));
    (dir, right, cross(dir, right))
}

fn fit_points(
    points: &[Vec3],
    bounds: &Bounds,
    view: &ViewDirection,
    vfov_degrees: f64,
    aspect: f64,
    margin: f64,
    aim: Vec3,
) -> f64 {
    let (dir, right, up) = basis(view);
    let tan_v = (vfov_degrees * std::f64::consts::PI / 360.0).tan() / margin;
    let tan_h = tan_v * aspect.max(1e-3);

    let mut distance = 0.0_f64;
    for corner in points {
        let offset = sub(*corner, aim);
        // Depth toward the camera: a corner nearer to it needs the camera
        // further back to keep the same corner inside the frustum.
        let z = dot(offset, dir);
        distance = distance
            .max(z + dot(offset, right).abs() / tan_h)
            .max(z + dot(offset, up).abs() / tan_v);
    }
    // A point, or a box flat in the view plane seen edge-on: still something
    // to stand back from.
    distance.max(radius_of(bounds) * 0.5).max(1e-3)
}

/// Distance from `center`, along `dir`, at which every point of the shape is
/// inside the camera's frustum with `margin` to spare on the tighter side.
/// Fitting points rather than the bounding sphere is what keeps a long, flat
/// model — a sword, a terrain tile — from sitting small in the middle of the
/// view.
pub fn fit_distance<'a>(
    shape: impl Into<Shape<'a>>,
    view: &ViewDirection,
    vfov_degrees: f64,
    aspect: f64,
    margin: f64,
    center: Option<Vec3>,
) -> f64 {
    let shape = shape.into();
    let points = shape.points();
    let bounds = shape.bounds();
    let aim = center.unwrap_or_else(|| center_of(&bounds));
    fit_points(&points, &bounds, view, vfov_degrees, aspect, margin, aim)
}

/// The margin the old viewer left around a framed model.
pub const DEFAULT_MARGIN: f64 = 1.12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Framing {
    /// What the camera looks at.
    pub target: Vec3,
    pub position: Vec3,
    pub distance: f64,
}

/// The view of the whole shape from `view`, centred on what the camera sees
/// rather than on the middle of the model: seen from above, a building's roof
/// and its forecourt project unevenly around its centre, and aiming at the
/// centre would leave the model sitting low in the frame with empty sky above
/// it. The aim is moved across the view plane until the projection is
/// balanced, then the distance fitted again; two rounds settle it.
pub fn frame_view<'a>(
    shape: impl Into<Shape<'a>>,
    view: &ViewDirection,
    vfov_degrees: f64,
    aspect: f64,
    margin: f64,
) -> Framing {
    let shape = shape.into();
    let points = shape.points();
    let bounds = shape.bounds();
    let (dir, right, up) = basis(view);
    let tan_v = (vfov_degrees * std::f64::consts::PI / 360.0).tan();
    let tan_h = tan_v * aspect.max(1e-3);
    let mut target = center_of(&bounds);
    let mut distance = fit_points(&points, &bounds, view, vfov_degrees, aspect, margin, target);
    for _ in 0..2 {
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for point in &points {
            let offset = sub(*point, target);
            let depth = (distance - dot(offset, dir)).max(1e-6);
            let x = dot(offset, right) / (depth * tan_h);
            let y = dot(offset, up) / (depth * tan_v);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        // Screen offsets back to world units at the target's depth.
        let shift_x = ((min_x + max_x) / 2.0) * distance * tan_h;
        let shift_y = ((min_y + max_y) / 2.0) * distance * tan_v;
        target = [
            target[0] + right[0] * shift_x + up[0] * shift_y,
            target[1] + right[1] * shift_x + up[1] * shift_y,
            target[2] + right[2] * shift_x + up[2] * shift_y,
        ];
        distance = fit_points(&points, &bounds, view, vfov_degrees, aspect, margin, target);
    }
    Framing {
        target,
        position: add(target, scale(dir, distance)),
        distance,
    }
}

/// Near and far planes for a camera `distance` from the centre of a model of
/// `radius`. Kept as tight as the view allows: depth precision is spread
/// between the two, and a near plane left at a tiny constant is what makes a
/// big model's coplanar faces flicker. The far plane leaves room for the
/// ground grid, which fades out well beyond the model.
pub fn clip_planes(distance: f64, radius: f64) -> (f64, f64) {
    let r = radius.max(1e-3);
    // A twentieth of the gap to the model's sphere, so the floor grid running
    // under the camera is not clipped either; zoomed into the model, a fixed
    // fraction of the distance.
    let near = (r * 1e-3).max(distance * 2e-3).max((distance - r) * 0.05);
    let far = distance + r * 60.0;
    (near, far)
}

/// The grid's spacing: a power of ten that puts the model across a handful of
/// major cells, each split in ten minor ones, so a 6 m hangar sits on a metre
/// grid and a 20 cm prop on a centimetre one.
pub fn grid_spacing(size: f64) -> (f64, f64) {
    let span = size.max(1e-4);
    let major = 10f64.powf((span / 2.0).log10().floor());
    (major / 10.0, major)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: Bounds = Bounds {
        min: [-0.5, -0.5, -0.5],
        max: [0.5, 0.5, 0.5],
    };

    fn len(v: Vec3) -> f64 {
        length(v)
    }

    /// Where a point lands in normalised device coordinates, for a camera
    /// built like the viewer's.
    fn project(
        point: Vec3,
        bounds: Bounds,
        angle: SnapshotAngle,
        fov: f64,
        aspect: f64,
    ) -> (f64, f64, f64) {
        let view = view_direction(angle);
        let eye = frame_view(bounds, &view, fov, aspect, DEFAULT_MARGIN).position;
        let dir = scale(view.dir, 1.0 / len(view.dir));
        let right = cross(view.up, dir);
        let r = scale(right, 1.0 / len(right));
        let up = cross(dir, r);
        let rel = sub(point, eye);
        let depth = -dot(rel, dir);
        let tan_v = (fov * std::f64::consts::PI / 360.0).tan();
        (
            dot(rel, r) / (depth * tan_v * aspect),
            dot(rel, up) / (depth * tan_v),
            depth,
        )
    }

    #[test]
    fn gives_a_unit_direction_and_an_up_that_is_not_along_it_for_every_angle() {
        for angle in SnapshotAngle::ALL {
            let view = view_direction(angle);
            assert!((len(view.dir) - 1.0).abs() < 1e-9, "{angle:?}");
            assert!(dot(view.dir, view.up).abs() < 0.99, "{angle:?}");
        }
    }

    #[test]
    fn puts_the_camera_in_front_on_z_on_the_right_on_x_above_on_y() {
        assert_eq!(view_direction(SnapshotAngle::Front).dir, [0.0, 0.0, 1.0]);
        assert_eq!(view_direction(SnapshotAngle::Right).dir, [1.0, 0.0, 0.0]);
        assert_eq!(view_direction(SnapshotAngle::Left).dir, [-1.0, 0.0, 0.0]);
        assert_eq!(view_direction(SnapshotAngle::Top).dir, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn draws_a_plan_with_the_models_front_at_the_bottom_of_the_image() {
        // Seen from above, a point at the front (+Z) projects below the centre.
        let (_, y, _) = project([0.0, 0.0, 0.5], UNIT, SnapshotAngle::Top, 35.0, 1.0);
        assert!(y < 0.0);
    }

    #[test]
    fn looks_at_the_iso_view_from_the_front_right_and_from_above() {
        let dir = view_direction(SnapshotAngle::Iso).dir;
        assert!(dir[0] > 0.0);
        assert!(dir[2] > 0.0);
        assert!((dir[1].asin() - std::f64::consts::FRAC_PI_6).abs() < 1e-9);
    }

    #[test]
    fn measures_the_azimuth_from_the_front_toward_x() {
        let side = direction_from(std::f64::consts::FRAC_PI_2, 0.0);
        assert!((side[0] - 1.0).abs() < 1e-9);
        assert!(side[2].abs() < 1e-9);
    }

    #[test]
    fn parses_the_angle_names_an_agent_uses() {
        assert_eq!("iso".parse(), Ok(SnapshotAngle::Iso));
        assert_eq!(" Top ".parse(), Ok(SnapshotAngle::Top));
        let error = "sideways".parse::<SnapshotAngle>().unwrap_err();
        assert!(error.contains("front, back, left, right, top, bottom, iso"));
    }

    #[test]
    fn keeps_every_corner_inside_the_frame_for_every_angle_and_shape() {
        let boxes = [
            UNIT,
            // A sword: long and thin.
            Bounds {
                min: [-0.05, 0.0, -1.2],
                max: [0.05, 0.1, 1.2],
            },
            // A terrain tile: wide and flat, off the origin.
            Bounds {
                min: [10.0, -0.2, 10.0],
                max: [30.0, 0.3, 26.0],
            },
        ];
        for bounds in boxes {
            for angle in SnapshotAngle::ALL {
                for aspect in [0.4, 1.0, 2.2] {
                    for corner in bounds.corners() {
                        let (x, y, depth) = project(corner, bounds, angle, 35.0, aspect);
                        assert!(depth > 0.0);
                        assert!(x.abs() <= 1.0 + 1e-9, "{angle:?} x {x}");
                        assert!(y.abs() <= 1.0 + 1e-9, "{angle:?} y {y}");
                    }
                }
            }
        }
    }

    #[test]
    fn fills_the_tighter_side_up_to_the_margin_not_the_whole_sphere() {
        let bounds = Bounds {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        let reach = bounds
            .corners()
            .into_iter()
            .map(|c| {
                let (x, y, _) = project(c, bounds, SnapshotAngle::Front, 35.0, 1.0);
                x.abs().max(y.abs())
            })
            .fold(0.0, f64::max);
        assert!((reach - 1.0 / 1.12).abs() < 0.01, "{reach}");
    }

    #[test]
    fn balances_the_projected_model_in_the_frame_instead_of_aiming_at_its_centre() {
        // A slab with a tall mast: seen from above at an angle, its middle is
        // nowhere near the middle of what the camera sees.
        let bounds = Bounds {
            min: [-6.0, -0.2, -5.0],
            max: [6.0, 5.0, 6.0],
        };
        for aspect in [0.5, 1.6] {
            let points: Vec<_> = bounds
                .corners()
                .into_iter()
                .map(|c| project(c, bounds, SnapshotAngle::Iso, 35.0, aspect))
                .collect();
            let (min_x, max_x) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.0), hi.max(p.0))
            });
            let (min_y, max_y) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.1), hi.max(p.1))
            });
            assert!((min_x + max_x).abs() < 0.02);
            assert!((min_y + max_y).abs() < 0.02);
        }
    }

    #[test]
    fn stands_further_back_in_a_narrow_panel_than_in_a_wide_one() {
        let view = view_direction(SnapshotAngle::Front);
        let wide = Bounds {
            min: [-2.0, -0.5, -0.5],
            max: [2.0, 0.5, 0.5],
        };
        assert!(
            fit_distance(wide, &view, 35.0, 0.5, DEFAULT_MARGIN, None)
                > fit_distance(wide, &view, 35.0, 2.0, DEFAULT_MARGIN, None)
        );
    }

    #[test]
    fn still_steps_back_from_a_single_point() {
        let point = Bounds {
            min: [1.0, 2.0, 3.0],
            max: [1.0, 2.0, 3.0],
        };
        let view = view_direction(SnapshotAngle::Iso);
        assert!(fit_distance(point, &view, 35.0, 1.0, DEFAULT_MARGIN, None) > 0.0);
    }

    #[test]
    fn frames_what_is_there_not_the_corners_of_its_box() {
        // A slab and one small thing high above its far corner: the box is
        // tall, the content is not, and the slab should not sit low under
        // empty sky.
        let slab = Bounds {
            min: [-6.0, -0.2, -5.0],
            max: [6.0, 0.0, 6.0],
        };
        let mut points = slab.corners().to_vec();
        points.extend([[4.0, 5.0, -4.0], [4.5, 5.2, -4.5]]);
        let bounds = Bounds {
            min: [-6.0, -0.2, -5.0],
            max: [6.0, 5.2, 6.0],
        };
        let view = view_direction(SnapshotAngle::Front);
        let from_points = frame_view(&points, &view, 35.0, 4.0 / 3.0, DEFAULT_MARGIN);
        let from_box = frame_view(bounds, &view, 35.0, 4.0 / 3.0, DEFAULT_MARGIN);
        // The box's empty top front edge, nearest the camera, projects highest
        // of all; without it the aim comes down onto the slab.
        assert!(from_points.target[1] < from_box.target[1] - 0.3);
    }

    #[test]
    fn still_keeps_every_point_in_the_frame() {
        let points: Vec<Vec3> = vec![
            [0.0, 0.0, 0.0],
            [3.0, 1.0, -2.0],
            [-1.0, 4.0, 1.0],
            [2.0, -1.0, 5.0],
        ];
        let view = view_direction(SnapshotAngle::Iso);
        let framing = frame_view(&points, &view, 35.0, 0.6, DEFAULT_MARGIN);
        let (dir, right, up) = basis(&view);
        let tan_v = (35.0 * std::f64::consts::PI / 360.0).tan();
        for p in &points {
            let rel = sub(*p, framing.position);
            let depth = -dot(rel, dir);
            assert!((dot(rel, right) / (depth * tan_v * 0.6)).abs() <= 1.0);
            assert!((dot(rel, up) / (depth * tan_v)).abs() <= 1.0);
        }
    }

    #[test]
    fn keeps_the_model_between_the_clip_planes() {
        for (distance, radius) in [(3.0, 1.0), (300.0, 100.0), (0.02, 0.01)] {
            let (near, far) = clip_planes(distance, radius);
            assert!(near > 0.0);
            let gap = distance - radius;
            assert!(near < if gap != 0.0 { gap } else { distance });
            assert!(far > distance + radius);
        }
    }

    #[test]
    fn keeps_the_depth_range_tight_enough_for_a_24_bit_buffer() {
        let (near, far) = clip_planes(30.0, 10.0);
        assert!(far / near < 5000.0);
    }

    #[test]
    fn never_reaches_zero_near_from_inside_the_model() {
        assert!(clip_planes(0.5, 10.0).0 > 0.0);
    }

    #[test]
    fn puts_a_few_major_cells_across_the_model() {
        let (minor, major) = grid_spacing(6.0);
        assert!((minor - 0.1).abs() < 1e-12 && (major - 1.0).abs() < 1e-12);
        assert!((grid_spacing(0.2).1 - 0.1).abs() < 1e-12);
        assert!((grid_spacing(2400.0).1 - 1000.0).abs() < 1e-9);
    }
}
