//! Animation: the keyframes a file holds, and the arithmetic that turns a time
//! into the value every channel has at that moment.
//!
//! Three parts, each in its own file:
//!
//! - this one: [`Clip`], its [`Channel`]s, and how a channel is sampled (step,
//!   linear and cubic-spline interpolation, quaternions turned along the
//!   shorter arc), which is what three.js's `AnimationMixer` did with a glTF's
//!   tracks;
//! - [`rig`]: the node graph the channels move, and the pose a clip puts it
//!   in at a time;
//! - [`deform`]: what a pose does to the vertices of a skinned or morphing
//!   mesh.
//!
//! Nothing here touches the GPU. A viewer holds a [`Playhead`] (which clip,
//! what time) and the renderer asks the model for the pose at it.

pub mod deform;
pub mod rig;

use glam::{Quat, Vec4};

pub use deform::{Deform, MorphTarget, VertexSkin};
pub use rig::{Attachment, Pose, Rig, RigNode, RigSkin};

/// How a channel gets from one keyframe to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Holds the earlier keyframe's value until the next one.
    Step,
    Linear,
    /// Hermite curves through the keyframes, with the tangents the file gives
    /// for each of them.
    CubicSpline,
}

/// What a channel moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Property {
    Translation,
    /// A quaternion, x y z w.
    Rotation,
    Scale,
    /// The weights of a mesh's morph targets, as many as it has targets.
    Weights,
}

/// The keyframes of one property of one node.
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// The glTF node it moves.
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    /// Seconds, strictly increasing in a valid file.
    pub times: Vec<f32>,
    /// `width` numbers for each keyframe — and three times that for a cubic
    /// spline, which gives an in-tangent, the value and an out-tangent in that
    /// order.
    pub values: Vec<f32>,
    /// Numbers in one value: 3 for a translation or a scale, 4 for a
    /// rotation, one for each morph target of a weights channel.
    pub width: usize,
}

impl Channel {
    /// Whether the numbers are the size the keyframes say. A channel that
    /// fails this is left out when the file is read, so sampling can index
    /// without checking.
    pub fn is_consistent(&self) -> bool {
        let per_key = self.width
            * if self.interpolation == Interpolation::CubicSpline {
                3
            } else {
                1
            };
        self.width > 0
            && !self.times.is_empty()
            && self.times.iter().all(|t| t.is_finite())
            && self.values.len() == self.times.len() * per_key
            && match self.property {
                Property::Translation | Property::Scale => self.width == 3,
                Property::Rotation => self.width == 4,
                Property::Weights => true,
            }
    }

    /// The value of keyframe `key` (for a cubic spline, the middle of its
    /// three numbers-groups).
    fn value(&self, key: usize) -> &[f32] {
        let at = if self.interpolation == Interpolation::CubicSpline {
            key * 3 + 1
        } else {
            key
        };
        &self.values[at * self.width..(at + 1) * self.width]
    }

    /// The channel's value at `time`, written into `out` (which is `width`
    /// numbers long). Before the first keyframe the first value holds, and
    /// after the last the last.
    pub fn sample(&self, time: f32, out: &mut [f32]) {
        let width = self.width;
        debug_assert_eq!(out.len(), width);
        let last = self.times.len() - 1;
        if !time.is_finite() || time <= self.times[0] || last == 0 {
            out.copy_from_slice(self.value(0));
            return;
        }
        if time >= self.times[last] {
            out.copy_from_slice(self.value(last));
            return;
        }
        // The keyframe at or before `time`: the last whose time is not after it.
        let key = self.times.partition_point(|&t| t <= time) - 1;
        let (t0, t1) = (self.times[key], self.times[key + 1]);
        let span = t1 - t0;
        if span <= 0.0 {
            out.copy_from_slice(self.value(key));
            return;
        }
        let u = (time - t0) / span;
        let rotation = self.property == Property::Rotation;
        match self.interpolation {
            Interpolation::Step => out.copy_from_slice(self.value(key)),
            Interpolation::Linear => {
                let (a, b) = (self.value(key), self.value(key + 1));
                if rotation {
                    let turned = quat(a).slerp(quat(b), u);
                    out.copy_from_slice(&turned.to_array());
                } else {
                    for ((o, &a), &b) in out.iter_mut().zip(a).zip(b) {
                        *o = a + (b - a) * u;
                    }
                }
            }
            Interpolation::CubicSpline => {
                // glTF 2.0 §3.11: values are [in-tangent, value, out-tangent]
                // per keyframe, and the tangents are per unit of the
                // keyframes' time, so they are scaled by the span.
                let (v0, v1) = (self.value(key), self.value(key + 1));
                let out_tangent = &self.values[(key * 3 + 2) * width..(key * 3 + 3) * width];
                let in_tangent = &self.values[(key + 1) * 3 * width..((key + 1) * 3 + 1) * width];
                let (u2, u3) = (u * u, u * u * u);
                let (h00, h10) = (2.0 * u3 - 3.0 * u2 + 1.0, u3 - 2.0 * u2 + u);
                let (h01, h11) = (-2.0 * u3 + 3.0 * u2, u3 - u2);
                for c in 0..width {
                    out[c] = h00 * v0[c]
                        + h10 * span * out_tangent[c]
                        + h01 * v1[c]
                        + h11 * span * in_tangent[c];
                }
                if rotation {
                    let turned = quat(out);
                    out.copy_from_slice(&turned.to_array());
                }
            }
        }
    }
}

/// A quaternion from four numbers, made a unit one so that an exporter's
/// rounding cannot skew a rotation; the identity when there is nothing to
/// normalise.
pub(crate) fn quat(xyzw: &[f32]) -> Quat {
    let v = Vec4::new(xyzw[0], xyzw[1], xyzw[2], xyzw[3]);
    if v.length_squared() > 1e-20 && v.is_finite() {
        Quat::from_vec4(v.normalize())
    } else {
        Quat::IDENTITY
    }
}

/// An animation the file holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub name: String,
    /// In seconds: the latest keyframe of any channel.
    pub duration: f32,
    pub channels: Vec<Channel>,
}

impl Clip {
    /// A clip that only lists: a name and a length, with nothing to play.
    pub fn new(name: impl Into<String>, duration: f32) -> Clip {
        Clip {
            name: name.into(),
            duration,
            channels: Vec::new(),
        }
    }
}

/// Where a viewer is in a model's animation: which clip, and how far in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Playhead {
    pub clip: usize,
    /// Seconds from the start of the clip.
    pub time: f32,
}

impl Playhead {
    /// The first frame of the first clip: how a model opens, and the picture
    /// an agent's snapshot is taken at.
    pub const START: Playhead = Playhead { clip: 0, time: 0.0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(
        property: Property,
        interpolation: Interpolation,
        width: usize,
        times: &[f32],
        values: &[f32],
    ) -> Channel {
        Channel {
            node: 0,
            property,
            interpolation,
            times: times.to_vec(),
            values: values.to_vec(),
            width,
        }
    }

    fn at(channel: &Channel, time: f32) -> Vec<f32> {
        let mut out = vec![0.0; channel.width];
        channel.sample(time, &mut out);
        out
    }

    fn close(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5)
    }

    #[test]
    fn blends_linearly_between_keyframes_and_holds_the_ends() {
        let c = channel(
            Property::Translation,
            Interpolation::Linear,
            3,
            &[1.0, 3.0],
            &[0.0, 0.0, 0.0, 4.0, 8.0, 2.0],
        );
        assert!(close(&at(&c, 0.0), &[0.0, 0.0, 0.0]), "before the first");
        assert!(close(&at(&c, 1.0), &[0.0, 0.0, 0.0]));
        assert!(close(&at(&c, 2.0), &[2.0, 4.0, 1.0]), "halfway");
        assert!(close(&at(&c, 2.5), &[3.0, 6.0, 1.5]));
        assert!(close(&at(&c, 3.0), &[4.0, 8.0, 2.0]));
        assert!(close(&at(&c, 99.0), &[4.0, 8.0, 2.0]), "after the last");
    }

    #[test]
    fn a_step_holds_the_earlier_value_until_the_next_keyframe() {
        let c = channel(
            Property::Scale,
            Interpolation::Step,
            3,
            &[0.0, 1.0, 2.0],
            &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0],
        );
        assert!(close(&at(&c, 0.99), &[1.0; 3]));
        assert!(close(&at(&c, 1.0), &[2.0; 3]));
        assert!(close(&at(&c, 1.99), &[2.0; 3]));
        assert!(close(&at(&c, 2.0), &[3.0; 3]));
    }

    #[test]
    fn picks_the_right_pair_among_many_keyframes() {
        let times: Vec<f32> = (0..10).map(|i| i as f32).collect();
        let values: Vec<f32> = times.iter().flat_map(|t| [*t * 10.0; 3]).collect();
        let c = channel(
            Property::Translation,
            Interpolation::Linear,
            3,
            &times,
            &values,
        );
        assert!(close(&at(&c, 6.25), &[62.5; 3]));
        assert!(close(&at(&c, 8.999), &[89.99; 3]));
    }

    #[test]
    fn turns_a_rotation_along_the_shorter_arc() {
        // A quarter turn about Y to its opposite: the half way is an eighth.
        let q = |angle: f32| Quat::from_rotation_y(angle).to_array();
        let (a, b) = (q(0.0), q(std::f32::consts::FRAC_PI_2));
        let c = channel(
            Property::Rotation,
            Interpolation::Linear,
            4,
            &[0.0, 1.0],
            &[a, b].concat(),
        );
        let half = at(&c, 0.5);
        assert!(close(&half, &q(std::f32::consts::FRAC_PI_4)), "{half:?}");

        // q and -q are the same turn: the blend between them does not spin
        // the long way round through zero.
        let negated: Vec<f32> = a.iter().map(|x| -x).collect();
        let c = channel(
            Property::Rotation,
            Interpolation::Linear,
            4,
            &[0.0, 1.0],
            &[a.to_vec(), negated].concat(),
        );
        let mid = quat(&at(&c, 0.5));
        assert!(mid.angle_between(Quat::IDENTITY) < 1e-4, "{mid:?}");
    }

    #[test]
    fn a_cubic_spline_follows_the_tangents_scaled_by_the_span() {
        // Values 0 and 1 over two seconds, with a flat in-tangent at the
        // start and a flat out-tangent at the end: the ease in and out curve.
        let c = channel(
            Property::Weights,
            Interpolation::CubicSpline,
            1,
            &[0.0, 2.0],
            &[
                0.0, 0.0, 0.0, // in, value, out of the first
                0.0, 1.0, 0.0, // in, value, out of the second
            ],
        );
        assert!(close(&at(&c, 0.0), &[0.0]));
        assert!(close(&at(&c, 1.0), &[0.5]), "symmetric about the middle");
        assert!(close(&at(&c, 2.0), &[1.0]));
        // A steady out-tangent of 1 per second over a span of 2 s makes the
        // curve lean forward. At a quarter of the way (u = 0.25) the Hermite
        // basis gives h10 = 0.140625 and h01 = 0.15625, so the value is
        // h10 * span * tangent + h01 * 1 = 0.28125 + 0.15625.
        let leaning = channel(
            Property::Weights,
            Interpolation::CubicSpline,
            1,
            &[0.0, 2.0],
            &[0.0, 0.0, 1.0, 0.0, 1.0, 0.0],
        );
        assert!(
            close(&at(&leaning, 0.5), &[0.4375]),
            "{:?}",
            at(&leaning, 0.5)
        );
    }

    #[test]
    fn a_cubic_rotation_stays_a_unit_quaternion() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let turned = Quat::from_rotation_z(1.0).to_array();
        let zero = [0.0; 4];
        let c = channel(
            Property::Rotation,
            Interpolation::CubicSpline,
            4,
            &[0.0, 1.0],
            &[zero, identity, zero, zero, turned, zero].concat(),
        );
        for step in 0..=10 {
            let q = at(&c, step as f32 / 10.0);
            let length = q.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((length - 1.0).abs() < 1e-4, "{q:?}");
        }
    }

    #[test]
    fn a_single_keyframe_is_a_constant_and_equal_times_do_not_divide_by_zero() {
        let one = channel(
            Property::Translation,
            Interpolation::Linear,
            3,
            &[5.0],
            &[1.0, 2.0, 3.0],
        );
        assert!(close(&at(&one, 0.0), &[1.0, 2.0, 3.0]));
        assert!(close(&at(&one, 9.0), &[1.0, 2.0, 3.0]));

        let twin = channel(
            Property::Translation,
            Interpolation::Linear,
            3,
            &[1.0, 1.0, 2.0],
            &[0.0; 3]
                .iter()
                .chain(&[4.0; 3])
                .chain(&[8.0; 3])
                .copied()
                .collect::<Vec<_>>(),
        );
        let v = at(&twin, 1.0);
        assert!(v.iter().all(|x| x.is_finite()), "{v:?}");
    }

    #[test]
    fn a_time_that_is_not_a_number_samples_the_first_keyframe() {
        let c = channel(
            Property::Scale,
            Interpolation::Linear,
            3,
            &[0.0, 1.0],
            &[1.0, 1.0, 1.0, 2.0, 2.0, 2.0],
        );
        assert!(close(&at(&c, f32::NAN), &[1.0; 3]));
    }

    #[test]
    fn spots_a_channel_whose_numbers_do_not_match_its_keyframes() {
        let good = channel(
            Property::Translation,
            Interpolation::Linear,
            3,
            &[0.0, 1.0],
            &[0.0; 6],
        );
        assert!(good.is_consistent());
        let short = Channel {
            values: vec![0.0; 5],
            ..good.clone()
        };
        assert!(!short.is_consistent());
        let cubic_short = Channel {
            interpolation: Interpolation::CubicSpline,
            ..good.clone()
        };
        assert!(!cubic_short.is_consistent(), "a spline needs three per key");
        let wrong_width = Channel {
            property: Property::Rotation,
            ..good.clone()
        };
        assert!(!wrong_width.is_consistent());
        let nan = Channel {
            times: vec![0.0, f32::NAN],
            ..good
        };
        assert!(!nan.is_consistent());
    }
}
