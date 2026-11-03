//! The orbit camera: what the mouse and the keyboard move, with the feel the
//! old viewer got from three.js's `OrbitControls` — damped rotation and
//! panning, zooming toward the cursor, +Y up, and a glide back to the framed
//! view.
//!
//! Plain arithmetic in f64, with no windowing in it: the widget (`viewer.rs`)
//! feeds it pixels and seconds, the tests feed it numbers.

use glam::{DVec3, Mat4, Vec3};

use crate::framing::{DEFAULT_MARGIN, ViewDirection, clip_planes, default_view, frame_view};
use crate::model::Model;
use crate::render::CameraMatrices;

/// Field of view: a long-ish lens, which flatters an object more than a wide
/// one.
pub const FOV_DEGREES: f64 = 35.0;

/// How far from the pole the camera may go, so "up" never flips.
const POLE_MARGIN: f64 = 1e-6;
/// Each frame at 60 Hz, this share of the motion still to come is applied.
const DAMPING: f64 = 0.12;
/// How long the camera takes to glide back when recentred.
pub const RECENTER_SECONDS: f64 = 0.42;
/// Panning smaller than this, in model units, is not worth another frame.
const PAN_EPSILON: f64 = 1e-9;
/// The camera may come this close to the target, and this far, in model
/// radii.
const MIN_DISTANCE: f64 = 0.02;
const MAX_DISTANCE: f64 = 40.0;

/// A glide from one view to another.
#[derive(Clone, Copy, Debug)]
struct Glide {
    from: (DVec3, DVec3),
    to: (DVec3, DVec3),
    elapsed: f64,
}

#[derive(Clone, Debug)]
pub struct Camera {
    pub eye: DVec3,
    pub target: DVec3,
    min_distance: f64,
    max_distance: f64,
    /// Rotation and panning still to be applied, eased in over the next
    /// frames rather than all at once.
    pending_theta: f64,
    pending_phi: f64,
    pending_pan: DVec3,
    glide: Option<Glide>,
    /// The camera is where framing put it, untouched by the user: a panel
    /// that changes shape — shown for the first time, expanded, dragged
    /// wider — frames the model again. The first drag or scroll ends that.
    pub auto_framed: bool,
}

/// A vector scaled to length one, or `fallback` for one with no direction.
fn unit_or(v: DVec3, fallback: DVec3) -> DVec3 {
    let length = v.length();
    if length > 1e-12 { v / length } else { fallback }
}

fn ease_out(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

impl Camera {
    /// The three-quarter view of a model, fitted to a viewport of `aspect`.
    pub fn framed(model: &Model, aspect: f64) -> Camera {
        let (eye, target) = default_pose(model, aspect);
        let mut camera = Camera {
            eye,
            target,
            min_distance: 0.0,
            max_distance: f64::INFINITY,
            pending_theta: 0.0,
            pending_phi: 0.0,
            pending_pan: DVec3::ZERO,
            glide: None,
            auto_framed: true,
        };
        camera.set_limits(model);
        camera
    }

    pub fn set_limits(&mut self, model: &Model) {
        self.min_distance = model.radius * MIN_DISTANCE;
        self.max_distance = model.radius * MAX_DISTANCE;
    }

    pub fn distance(&self) -> f64 {
        (self.eye - self.target).length()
    }

    /// The camera's right, up and forward axes. Up on screen is +Y whenever it
    /// can be.
    pub fn axes(&self) -> (DVec3, DVec3, DVec3) {
        let forward = unit_or(self.target - self.eye, DVec3::NEG_Z);
        let mut right = forward.cross(DVec3::Y);
        if right.length_squared() < 1e-18 {
            right = DVec3::X;
        }
        let right = right.normalize();
        (right, right.cross(forward), forward)
    }

    /// A hand on the camera ends a glide rather than fighting it, and means
    /// the view is no longer the automatic one.
    pub fn take_control(&mut self) {
        self.glide = None;
        self.auto_framed = false;
    }

    /// Turns the camera around its target by angles in radians: `left` moves
    /// it toward the viewer's left, `up` toward the top. Eased in.
    pub fn rotate(&mut self, left: f64, up: f64) {
        self.take_control();
        self.pending_theta -= left;
        self.pending_phi -= up;
    }

    /// Slides the target across the view by pixels dragged, in a viewport
    /// `height` pixels tall: the model follows the pointer. Eased in.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64, height: f64) {
        self.take_control();
        let (right, up, _) = self.axes();
        let per_pixel =
            2.0 * self.distance() * (FOV_DEGREES.to_radians() / 2.0).tan() / height.max(1.0);
        self.pending_pan += -right * (dx * per_pixel) + up * (dy * per_pixel);
    }

    /// Moves the camera along its line of sight to `scale` times the distance.
    pub fn dolly(&mut self, scale: f64) {
        self.take_control();
        let distance = self.distance();
        let next = (distance * scale).clamp(self.min_distance, self.max_distance);
        let direction = unit_or(self.eye - self.target, DVec3::Z);
        self.eye = self.target + direction * next;
    }

    /// Dollies along the ray through a point of the viewport, so the thing
    /// under the pointer stays under it. `ndc` is the pointer in normalised
    /// device coordinates (x right, y up, both in [-1, 1]).
    pub fn dolly_to(&mut self, scale: f64, ndc: (f64, f64), aspect: f64) {
        self.take_control();
        let (right, up, forward) = self.axes();
        let tan_v = (FOV_DEGREES.to_radians() / 2.0).tan();
        let ray = (right * (ndc.0 * tan_v * aspect) + up * (ndc.1 * tan_v) + forward).normalize();
        let before = self.distance();
        let after = (before * scale).clamp(self.min_distance, self.max_distance);
        // Moves down the pointer's ray, then looks the same way from there:
        // the target lands in front of the new position.
        self.eye += ray * (before - after);
        self.target = self.eye + forward * after;
    }

    /// Whether any motion is still to play out: damped input, or a glide.
    pub fn is_moving(&self) -> bool {
        self.glide.is_some()
            || self.pending_theta.abs() > 1e-6
            || self.pending_phi.abs() > 1e-6
            || self.pending_pan.length() > PAN_EPSILON
    }

    /// Plays `seconds` of the motion that is under way.
    pub fn advance(&mut self, seconds: f64) {
        if let Some(mut glide) = self.glide {
            glide.elapsed += seconds;
            let t = (glide.elapsed / RECENTER_SECONDS).min(1.0);
            let k = ease_out(t);
            self.eye = glide.from.0.lerp(glide.to.0, k);
            self.target = glide.from.1.lerp(glide.to.1, k);
            self.glide = (t < 1.0).then_some(glide);
            return;
        }
        // The share of what is left that this slice of time applies: 12% a
        // frame at 60 Hz, whatever the real frame rate.
        let k = 1.0 - (1.0 - DAMPING).powf(seconds * 60.0);
        let offset = self.eye - self.target;
        let radius = offset.length().max(1e-12);
        let mut theta = offset.x.atan2(offset.z);
        let mut phi = (offset.y / radius).clamp(-1.0, 1.0).acos();
        theta += self.pending_theta * k;
        phi = (phi + self.pending_phi * k).clamp(POLE_MARGIN, std::f64::consts::PI - POLE_MARGIN);
        self.pending_theta *= 1.0 - k;
        self.pending_phi *= 1.0 - k;
        self.target += self.pending_pan * k;
        self.pending_pan *= 1.0 - k;
        if self.pending_theta.abs() < 1e-7 {
            self.pending_theta = 0.0;
        }
        if self.pending_phi.abs() < 1e-7 {
            self.pending_phi = 0.0;
        }
        if self.pending_pan.length() < PAN_EPSILON {
            self.pending_pan = DVec3::ZERO;
        }
        let radius = radius.clamp(self.min_distance, self.max_distance);
        self.eye = self.target
            + DVec3::new(
                radius * phi.sin() * theta.sin(),
                radius * phi.cos(),
                radius * phi.sin() * theta.cos(),
            );
    }

    /// Glides back to the automatic view, or jumps there.
    pub fn recenter(&mut self, model: &Model, aspect: f64, animate: bool) {
        let to = default_pose(model, aspect);
        self.pending_theta = 0.0;
        self.pending_phi = 0.0;
        self.pending_pan = DVec3::ZERO;
        self.auto_framed = true;
        self.set_limits(model);
        if animate {
            self.glide = Some(Glide {
                from: (self.eye, self.target),
                to,
                elapsed: 0.0,
            });
        } else {
            self.glide = None;
            (self.eye, self.target) = to;
        }
    }

    /// The view of the model from where the camera is now, for a viewport
    /// of `aspect`.
    pub(crate) fn matrices(&self, aspect: f64, radius: f64) -> CameraMatrices {
        let (near, far) = clip_planes(self.distance(), radius);
        let view = Mat4::look_at_rh(self.eye.as_vec3(), self.target.as_vec3(), Vec3::Y);
        let projection = Mat4::perspective_rh(
            FOV_DEGREES.to_radians() as f32,
            aspect.max(1e-3) as f32,
            near as f32,
            far as f32,
        );
        CameraMatrices {
            view_proj: projection * view,
            eye: self.eye.as_vec3().to_array(),
        }
    }
}

/// Eye and target of the default view of a model.
fn default_pose(model: &Model, aspect: f64) -> (DVec3, DVec3) {
    let framing = frame_view(
        &model.hull,
        &default_view(),
        FOV_DEGREES,
        aspect,
        DEFAULT_MARGIN,
    );
    (DVec3::from(framing.position), DVec3::from(framing.target))
}

/// The view an agent's snapshot is taken from: the model framed from a
/// canonical direction, for a picture of `aspect`.
pub(crate) fn snapshot_matrices(
    model: &Model,
    view: &ViewDirection,
    aspect: f64,
) -> CameraMatrices {
    let framing = frame_view(&model.hull, view, FOV_DEGREES, aspect, DEFAULT_MARGIN);
    let (near, far) = clip_planes(framing.distance, model.radius);
    let eye = DVec3::from(framing.position).as_vec3();
    let target = DVec3::from(framing.target).as_vec3();
    let up = DVec3::from(view.up).as_vec3();
    let projection = Mat4::perspective_rh(
        FOV_DEGREES.to_radians() as f32,
        aspect.max(1e-3) as f32,
        near as f32,
        far as f32,
    );
    CameraMatrices {
        view_proj: projection * Mat4::look_at_rh(eye, target, up),
        eye: eye.to_array(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::model::ModelParts;
    use crate::model::testing::{cube_mesh, standing};

    fn cube() -> Model {
        Model::new(
            "cube",
            ModelFormat::Glb,
            ModelParts {
                meshes: vec![cube_mesh(None)],
                instances: vec![standing(0, Mat4::IDENTITY)],
                ..ModelParts::default()
            },
        )
    }

    fn settle(camera: &mut Camera) {
        for _ in 0..600 {
            camera.advance(1.0 / 60.0);
        }
    }

    #[test]
    fn opens_on_the_three_quarter_view_looking_at_the_model() {
        let model = cube();
        let camera = Camera::framed(&model, 4.0 / 3.0);
        assert!(camera.auto_framed);
        // From the front right and above.
        let offset = camera.eye - camera.target;
        assert!(
            offset.x > 0.0 && offset.y > 0.0 && offset.z > 0.0,
            "{offset:?}"
        );
        assert!(camera.target.length() < 0.2, "{:?}", camera.target);
        assert!(camera.distance() > model.radius);
    }

    #[test]
    fn turns_around_the_target_keeping_its_distance() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let distance = camera.distance();
        let before = camera.eye - camera.target;
        camera.rotate(0.5, 0.0);
        assert!(!camera.auto_framed);
        assert!(camera.is_moving());
        settle(&mut camera);
        assert!(!camera.is_moving());
        assert!((camera.distance() - distance).abs() < 1e-9);
        let after = camera.eye - camera.target;
        // Dragging toward the left turns the azimuth down by the angle.
        let turned = before.x.atan2(before.z) - after.x.atan2(after.z);
        assert!((turned - 0.5).abs() < 1e-3, "{turned}");
        assert!((before.y - after.y).abs() < 1e-9);
    }

    #[test]
    fn eases_toward_the_new_view_instead_of_jumping() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let start = camera.eye;
        camera.rotate(1.0, 0.0);
        camera.advance(1.0 / 60.0);
        let first = (camera.eye - start).length();
        settle(&mut camera);
        let total = (camera.eye - start).length();
        assert!(first > 0.0 && first < total * 0.2, "{first} of {total}");
    }

    #[test]
    fn never_goes_over_the_pole() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let distance = camera.distance();
        // Dragging far down takes the camera to the bottom pole and no
        // further: it stays under the model, not flipped over it.
        camera.rotate(0.0, -10.0);
        settle(&mut camera);
        let offset = camera.eye - camera.target;
        assert!(
            offset.y < 0.0 && offset.x.hypot(offset.z) < distance * 1e-3,
            "{offset:?}"
        );
        // And far up to the top one.
        camera.rotate(0.0, 20.0);
        settle(&mut camera);
        let offset = camera.eye - camera.target;
        assert!(
            offset.y > 0.0 && offset.x.hypot(offset.z) < distance * 1e-3,
            "{offset:?}"
        );
        assert!((offset.length() - distance).abs() < 1e-9);
    }

    #[test]
    fn pans_the_target_with_the_pointer() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let (right, up, _) = camera.axes();
        let before = camera.target;
        camera.pan_pixels(100.0, 0.0, 600.0);
        settle(&mut camera);
        // The pointer went right, so the target went left.
        assert!((camera.target - before).dot(right) < -0.01);
        assert!((camera.target - before).dot(up).abs() < 1e-9);
        let before = camera.target;
        camera.pan_pixels(0.0, 100.0, 600.0);
        settle(&mut camera);
        assert!((camera.target - before).dot(up) > 0.01);
    }

    #[test]
    fn keeps_the_distance_between_the_limits_when_zooming() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        for _ in 0..200 {
            camera.dolly(0.8);
        }
        assert!((camera.distance() - model.radius * 0.02).abs() < 1e-9);
        for _ in 0..200 {
            camera.dolly(1.25);
        }
        assert!((camera.distance() - model.radius * 40.0).abs() < 1e-6);
    }

    #[test]
    fn zooms_along_the_ray_through_the_pointer() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.5);
        let (_, _, forward) = camera.axes();
        let ndc = (0.6, -0.3);
        let (right, up, _) = camera.axes();
        let tan_v = (FOV_DEGREES.to_radians() / 2.0).tan();
        let ray = (right * (ndc.0 * tan_v * 1.5) + up * (ndc.1 * tan_v) + forward).normalize();
        let before = camera.eye;
        let distance = camera.distance();
        camera.dolly_to(0.5, ndc, 1.5);
        let moved = camera.eye - before;
        assert!(moved.normalize().dot(ray) > 0.999_999, "{moved:?}");
        assert!((moved.length() - distance * 0.5).abs() < 1e-9);
        // Still looking the same way, at half the distance.
        let (_, _, after) = camera.axes();
        assert!((after - forward).length() < 1e-9);
        assert!((camera.distance() - distance * 0.5).abs() < 1e-9);
    }

    #[test]
    fn zooms_straight_in_when_the_pointer_is_at_the_centre() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let before = camera.target;
        camera.dolly_to(0.5, (0.0, 0.0), 1.0);
        assert!((camera.target - before).length() < 1e-9);
    }

    #[test]
    fn glides_back_to_the_framed_view() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let home = (camera.eye, camera.target);
        camera.rotate(1.0, 0.4);
        camera.pan_pixels(50.0, 20.0, 400.0);
        settle(&mut camera);
        assert!((camera.eye - home.0).length() > 0.1);
        camera.recenter(&model, 1.0, true);
        assert!(camera.auto_framed && camera.is_moving());
        camera.advance(RECENTER_SECONDS * 0.5);
        let midway = (camera.eye - home.0).length();
        assert!(midway > 1e-6, "still on the way");
        camera.advance(RECENTER_SECONDS);
        assert!(!camera.is_moving());
        assert!((camera.eye - home.0).length() < 1e-9);
        assert!((camera.target - home.1).length() < 1e-9);
    }

    #[test]
    fn jumps_back_when_motion_is_not_wanted() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        let home = camera.eye;
        camera.dolly(0.3);
        camera.recenter(&model, 1.0, false);
        assert!(!camera.is_moving());
        assert!((camera.eye - home).length() < 1e-12);
    }

    #[test]
    fn a_hand_on_the_camera_ends_a_glide() {
        let model = cube();
        let mut camera = Camera::framed(&model, 1.0);
        camera.rotate(1.0, 0.0);
        settle(&mut camera);
        camera.recenter(&model, 1.0, true);
        camera.dolly(0.9);
        assert!(!camera.auto_framed);
        camera.advance(1.0);
        assert!(!camera.is_moving());
    }

    #[test]
    fn projects_the_target_to_the_middle_of_the_picture() {
        let model = cube();
        let camera = Camera::framed(&model, 2.0);
        let matrices = camera.matrices(2.0, model.radius);
        let target = matrices.view_proj * camera.target.as_vec3().extend(1.0);
        let ndc = target.truncate() / target.w;
        assert!(ndc.x.abs() < 1e-5 && ndc.y.abs() < 1e-5, "{ndc:?}");
        assert!(ndc.z > 0.0 && ndc.z < 1.0, "depth {ndc:?}");
    }

    #[test]
    fn keeps_every_corner_of_the_model_in_the_picture() {
        let model = cube();
        for aspect in [0.5, 1.0, 2.0] {
            let camera = Camera::framed(&model, aspect);
            let matrices = camera.matrices(aspect, model.radius);
            for corner in model.bounds.corners() {
                let clip = matrices.view_proj * Vec3::from(corner.map(|c| c as f32)).extend(1.0);
                let ndc = clip.truncate() / clip.w;
                assert!(
                    ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
                    "{aspect}: {ndc:?}"
                );
            }
        }
    }

    #[test]
    fn takes_a_snapshot_from_the_side_the_agent_asked() {
        use crate::framing::{SnapshotAngle, view_direction};
        let model = cube();
        let front = snapshot_matrices(&model, &view_direction(SnapshotAngle::Front), 1.0);
        assert!(front.eye[2] > 1.0 && front.eye[0].abs() < 1e-4 && front.eye[1].abs() < 1e-4);
        let top = snapshot_matrices(&model, &view_direction(SnapshotAngle::Top), 1.0);
        assert!(top.eye[1] > 1.0);
    }
}
