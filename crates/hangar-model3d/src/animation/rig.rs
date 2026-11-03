//! The node graph a clip moves: each node's place among its parents, the skins
//! that tie joints to meshes, and which node each instance of the model stands
//! on. A clip is applied to it for a time ([`Rig::pose`]) and the result tells
//! where every instance, and every joint, is at that moment.

use glam::{Mat4, Quat, Vec3};

use super::{Clip, Property, quat};

/// One node of the file, as animation needs it.
#[derive(Clone, Debug)]
pub struct RigNode {
    pub parent: Option<usize>,
    /// The node's matrix as the file has it: kept as it is, with whatever
    /// shear a matrix can hold, for every node no channel moves.
    pub local: Mat4,
    /// The same, taken apart: what a channel replaces one part of.
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

/// A skin: the nodes that move its mesh, and the matrices that undo the pose
/// the mesh was bound in.
#[derive(Clone, Debug)]
pub struct RigSkin {
    pub joints: Vec<usize>,
    /// One for each joint; a joint that has none stays where it is.
    pub inverse_bind: Vec<Mat4>,
}

/// Where one instance of the model stands in the graph.
#[derive(Clone, Debug)]
pub struct Attachment {
    /// The node that carries the instance's mesh.
    pub node: usize,
    /// The skin that moves its vertices, if it has one. A skinned mesh
    /// ignores the transform of its node: the joints place it.
    pub skin: Option<usize>,
    /// The morph weights the mesh starts with (the node's, or else the
    /// mesh's own).
    pub weights: Vec<f32>,
}

/// The graph, its skins and the placement of every instance of the model.
#[derive(Clone, Debug)]
pub struct Rig {
    nodes: Vec<RigNode>,
    skins: Vec<RigSkin>,
    /// One for each instance of the model, in the same order.
    attachments: Vec<Attachment>,
    /// Node indices, parents before children.
    order: Vec<usize>,
}

/// Every node's place at one moment.
#[derive(Clone, Debug)]
pub struct Pose {
    /// Node to the file's coordinates, for every node.
    pub world: Vec<Mat4>,
    /// The morph weights a clip sets, for the nodes it sets them on.
    weights: Vec<Option<Vec<f32>>>,
}

impl Rig {
    pub fn new(nodes: Vec<RigNode>, skins: Vec<RigSkin>, attachments: Vec<Attachment>) -> Rig {
        let order = parents_first(&nodes);
        Rig {
            nodes,
            skins,
            attachments,
            order,
        }
    }

    pub fn attachments(&self) -> &[Attachment] {
        &self.attachments
    }

    pub fn skins(&self) -> &[RigSkin] {
        &self.skins
    }

    pub fn nodes(&self) -> &[RigNode] {
        &self.nodes
    }

    /// Everything's place when `at` (a clip and a time in it) is applied to
    /// the nodes as the file leaves them, or just as the file leaves them
    /// with `None`.
    pub fn pose(&self, at: Option<(&Clip, f32)>) -> Pose {
        let count = self.nodes.len();
        let mut world: Vec<Mat4> = self.nodes.iter().map(|node| node.local).collect();
        let mut weights: Vec<Option<Vec<f32>>> = vec![None; count];

        if let Some((clip, time)) = at {
            // A node a channel moves is put together from its parts again,
            // with the parts nothing moves as the file has them.
            let mut moved: Vec<Option<(Vec3, Quat, Vec3)>> = vec![None; count];
            let mut value = [0.0f32; 4];
            for channel in &clip.channels {
                let Some(node) = self.nodes.get(channel.node) else {
                    continue;
                };
                let parts = || (node.translation, node.rotation, node.scale);
                match channel.property {
                    Property::Translation => {
                        channel.sample(time, &mut value[..3]);
                        moved[channel.node].get_or_insert_with(parts).0 =
                            Vec3::new(value[0], value[1], value[2]);
                    }
                    Property::Rotation => {
                        channel.sample(time, &mut value);
                        moved[channel.node].get_or_insert_with(parts).1 = quat(&value);
                    }
                    Property::Scale => {
                        channel.sample(time, &mut value[..3]);
                        moved[channel.node].get_or_insert_with(parts).2 =
                            Vec3::new(value[0], value[1], value[2]);
                    }
                    Property::Weights => {
                        let mut set = vec![0.0; channel.width];
                        channel.sample(time, &mut set);
                        weights[channel.node] = Some(set);
                    }
                }
            }
            for (matrix, moved) in world.iter_mut().zip(&moved) {
                if let Some((translation, rotation, scale)) = moved {
                    *matrix =
                        Mat4::from_scale_rotation_translation(*scale, *rotation, *translation);
                }
            }
        }

        // `world` holds the local matrices; a node's parent comes first in
        // `order`, so by the time the node is reached its parent's is final.
        for &index in &self.order {
            if let Some(parent) = self.nodes[index].parent {
                world[index] = world[parent] * world[index];
            }
        }
        Pose { world, weights }
    }

    /// Where an instance stands: its node's matrix, or nothing at all for a
    /// skinned one, whose vertices the joints already placed.
    pub fn instance_matrix(&self, instance: usize, pose: &Pose) -> Mat4 {
        match self.attachments.get(instance) {
            Some(Attachment {
                skin: None, node, ..
            }) => pose.world.get(*node).copied().unwrap_or(Mat4::IDENTITY),
            _ => Mat4::IDENTITY,
        }
    }

    /// The matrices that pose a skin: each joint in the file's coordinates,
    /// times the inverse of the pose the mesh was bound in.
    pub fn joint_matrices(&self, skin: usize, pose: &Pose) -> Vec<Mat4> {
        let Some(skin) = self.skins.get(skin) else {
            return Vec::new();
        };
        skin.joints
            .iter()
            .enumerate()
            .map(|(i, &joint)| {
                pose.world.get(joint).copied().unwrap_or(Mat4::IDENTITY)
                    * skin.inverse_bind.get(i).copied().unwrap_or(Mat4::IDENTITY)
            })
            .collect()
    }

    /// The morph weights of an instance in a pose: those a clip sets on its
    /// node, or else the ones it starts with.
    pub fn weights<'a>(&'a self, instance: usize, pose: &'a Pose) -> &'a [f32] {
        let Some(attachment) = self.attachments.get(instance) else {
            return &[];
        };
        pose.weights
            .get(attachment.node)
            .and_then(Option::as_deref)
            .unwrap_or(&attachment.weights)
    }
}

/// Node indices with every parent before its children. A file with a cycle in
/// its nodes is invalid, but not a reason to loop: the nodes in it are
/// treated as roots, each once.
fn parents_first(nodes: &[RigNode]) -> Vec<usize> {
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent.filter(|&p| p < nodes.len() && p != index) {
            children[parent].push(index);
        }
    }
    let mut order = Vec::with_capacity(nodes.len());
    let mut seen = vec![false; nodes.len()];
    let mut stack: Vec<usize> = Vec::new();
    let roots = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent.is_none_or(|p| p >= nodes.len()))
        .map(|(index, _)| index);
    // Nodes in a cycle have a parent and are nobody's root: they come after
    // the real roots, so those are never reordered by them.
    let rest = 0..nodes.len();
    for start in roots.chain(rest) {
        if seen[start] {
            continue;
        }
        stack.push(start);
        while let Some(index) = stack.pop() {
            if std::mem::replace(&mut seen[index], true) {
                continue;
            }
            order.push(index);
            stack.extend(children[index].iter().rev());
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{Channel, Interpolation};

    fn node(parent: Option<usize>, translation: [f32; 3]) -> RigNode {
        let t = Vec3::from(translation);
        RigNode {
            parent,
            local: Mat4::from_translation(t),
            translation: t,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    fn slide(node: usize, to: [f32; 3]) -> Channel {
        Channel {
            node,
            property: Property::Translation,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: [[0.0; 3], to].concat(),
            width: 3,
        }
    }

    fn clip(channels: Vec<Channel>) -> Clip {
        Clip {
            name: "clip".into(),
            duration: 1.0,
            channels,
        }
    }

    fn place(rig: &Rig, pose: &Pose, node: usize) -> Vec3 {
        let _ = rig;
        pose.world[node].transform_point3(Vec3::ZERO)
    }

    #[test]
    fn a_node_stands_where_its_parents_put_it() {
        let rig = Rig::new(
            vec![
                node(None, [1.0, 0.0, 0.0]),
                node(Some(0), [0.0, 2.0, 0.0]),
                node(Some(1), [0.0, 0.0, 3.0]),
            ],
            vec![],
            vec![],
        );
        let pose = rig.pose(None);
        assert_eq!(place(&rig, &pose, 2), Vec3::new(1.0, 2.0, 3.0));
    }

    #[test]
    fn works_out_parents_even_when_the_file_lists_children_first() {
        let rig = Rig::new(
            vec![node(Some(1), [0.0, 5.0, 0.0]), node(None, [1.0, 0.0, 0.0])],
            vec![],
            vec![],
        );
        assert_eq!(place(&rig, &rig.pose(None), 0), Vec3::new(1.0, 5.0, 0.0));
    }

    #[test]
    fn a_clip_moves_a_node_and_everything_under_it() {
        let rig = Rig::new(
            vec![node(None, [0.0; 3]), node(Some(0), [0.0, 1.0, 0.0])],
            vec![],
            vec![],
        );
        let moving = clip(vec![slide(0, [4.0, 0.0, 0.0])]);
        let pose = rig.pose(Some((&moving, 0.5)));
        assert_eq!(place(&rig, &pose, 0), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(place(&rig, &pose, 1), Vec3::new(2.0, 1.0, 0.0));
        // At the start of the clip, the first keyframe.
        let pose = rig.pose(Some((&moving, 0.0)));
        assert_eq!(place(&rig, &pose, 1), Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn a_channel_leaves_the_parts_it_does_not_move_as_the_file_has_them() {
        let mut turned = node(None, [7.0, 0.0, 0.0]);
        turned.rotation = Quat::from_rotation_z(0.5);
        turned.scale = Vec3::new(2.0, 2.0, 2.0);
        turned.local = Mat4::from_scale_rotation_translation(
            turned.scale,
            turned.rotation,
            turned.translation,
        );
        let rig = Rig::new(vec![turned], vec![], vec![]);
        let moving = clip(vec![slide(0, [0.0, 3.0, 0.0])]);
        let pose = rig.pose(Some((&moving, 1.0)));
        let (scale, rotation, translation) = pose.world[0].to_scale_rotation_translation();
        assert!((translation - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-5);
        assert!((scale - Vec3::splat(2.0)).length() < 1e-5);
        assert!(rotation.angle_between(Quat::from_rotation_z(0.5)) < 1e-5);
    }

    #[test]
    fn a_node_no_channel_moves_keeps_its_matrix_whole() {
        // A shear cannot be taken apart into translation, rotation and
        // scale; a node nothing animates is not taken apart.
        let shear = Mat4::from_cols_array(&[
            1.0, 0.0, 0.0, 0.0, //
            0.5, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ]);
        let mut sheared = node(None, [0.0; 3]);
        sheared.local = shear;
        let rig = Rig::new(vec![sheared, node(None, [0.0; 3])], vec![], vec![]);
        let moving = clip(vec![slide(1, [1.0, 0.0, 0.0])]);
        let pose = rig.pose(Some((&moving, 1.0)));
        assert_eq!(pose.world[0], shear);
    }

    #[test]
    fn ignores_a_channel_for_a_node_that_is_not_there() {
        let rig = Rig::new(vec![node(None, [1.0, 0.0, 0.0])], vec![], vec![]);
        let stray = clip(vec![slide(9, [5.0, 5.0, 5.0])]);
        let pose = rig.pose(Some((&stray, 1.0)));
        assert_eq!(place(&rig, &pose, 0), Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_cycle_in_the_nodes_does_not_loop() {
        let rig = Rig::new(
            vec![
                node(Some(1), [1.0, 0.0, 0.0]),
                node(Some(0), [0.0, 1.0, 0.0]),
            ],
            vec![],
            vec![],
        );
        // Whatever the answer, every node is posed once and it ends.
        assert_eq!(rig.order.len(), 2);
        let pose = rig.pose(None);
        assert_eq!(pose.world.len(), 2);
    }

    #[test]
    fn a_joint_matrix_is_the_joint_in_the_world_times_the_inverse_bind() {
        let rig = Rig::new(
            vec![node(None, [0.0; 3]), node(Some(0), [0.0, 1.0, 0.0])],
            vec![RigSkin {
                joints: vec![0, 1],
                inverse_bind: vec![
                    Mat4::IDENTITY,
                    Mat4::from_translation(Vec3::new(0.0, -1.0, 0.0)),
                ],
            }],
            vec![],
        );
        // In the pose the mesh was bound in, a joint matrix does nothing.
        let rest = rig.joint_matrices(0, &rig.pose(None));
        assert!(rest[1].abs_diff_eq(Mat4::IDENTITY, 1e-6));
        // Slide the root: both joints carry the mesh along.
        let moving = clip(vec![slide(0, [3.0, 0.0, 0.0])]);
        let posed = rig.joint_matrices(0, &rig.pose(Some((&moving, 1.0))));
        assert_eq!(
            posed[1].transform_point3(Vec3::new(0.0, 1.0, 0.0)),
            Vec3::new(3.0, 1.0, 0.0)
        );
        assert!(rig.joint_matrices(5, &rig.pose(None)).is_empty());
    }

    #[test]
    fn a_skinned_instance_stays_at_the_origin_and_the_others_follow_their_node() {
        let attach = |node, skin| Attachment {
            node,
            skin,
            weights: vec![],
        };
        let rig = Rig::new(
            vec![node(None, [2.0, 0.0, 0.0])],
            vec![RigSkin {
                joints: vec![0],
                inverse_bind: vec![],
            }],
            vec![attach(0, None), attach(0, Some(0))],
        );
        let pose = rig.pose(None);
        assert_eq!(
            rig.instance_matrix(0, &pose).transform_point3(Vec3::ZERO),
            Vec3::new(2.0, 0.0, 0.0)
        );
        assert_eq!(rig.instance_matrix(1, &pose), Mat4::IDENTITY);
        assert_eq!(rig.instance_matrix(7, &pose), Mat4::IDENTITY);
    }

    #[test]
    fn morph_weights_come_from_the_clip_or_else_from_the_file() {
        let rig = Rig::new(
            vec![node(None, [0.0; 3])],
            vec![],
            vec![Attachment {
                node: 0,
                skin: None,
                weights: vec![0.25, 0.0],
            }],
        );
        assert_eq!(rig.weights(0, &rig.pose(None)), [0.25, 0.0]);
        let blink = clip(vec![Channel {
            node: 0,
            property: Property::Weights,
            interpolation: Interpolation::Linear,
            times: vec![0.0, 1.0],
            values: vec![0.0, 0.0, 1.0, 0.5],
            width: 2,
        }]);
        assert_eq!(rig.weights(0, &rig.pose(Some((&blink, 0.5)))), [0.5, 0.25]);
        assert!(rig.weights(3, &rig.pose(None)).is_empty());
    }
}
