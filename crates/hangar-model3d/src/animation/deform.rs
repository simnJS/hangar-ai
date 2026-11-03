//! What moves a mesh's own vertices: the weights of its morph targets, then the
//! joints of its skin.
//!
//! The renderer does it on the processor and uploads the result, rather than
//! in a vertex shader. That is a choice of this viewer: the wireframe and the
//! normals are drawn from the same vertex buffer as the shaded model, so they
//! follow an animation with no second path, and one pipeline serves every
//! mesh. The price is a pass over the vertices each frame, which is spread
//! over the machine's cores for the meshes where it would show.

use glam::{Mat4, Vec3};

use crate::model::Vertex;

/// One morph target: how far it moves each vertex at weight one.
#[derive(Clone, Debug, PartialEq)]
pub struct MorphTarget {
    /// Per vertex; empty when the target moves no positions.
    pub positions: Vec<[f32; 3]>,
    /// Per vertex; empty when the target changes no normals.
    pub normals: Vec<[f32; 3]>,
}

/// Which joints move a vertex, and how much each does.
#[derive(Clone, Debug, PartialEq)]
pub struct VertexSkin {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// What a mesh keeps for moving its vertices after it was read. The mesh's
/// own vertices are the file's rest pose; this is the part that is not drawn
/// as it stands.
#[derive(Clone, Debug, PartialEq)]
pub struct Deform {
    /// Where each vertex is before any target or joint acts on it.
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub skin: Option<VertexSkin>,
    pub targets: Vec<MorphTarget>,
    /// The file gave the mesh no normals, so it is drawn faceted: three
    /// vertices to a triangle, each triangle with the normal of its face, and
    /// the normals are worked out again from the moved triangles.
    pub faceted: bool,
}

/// Vertices below this many are moved on the calling thread: starting threads
/// costs more than the work.
const THREADED_FROM: usize = 20_000;

impl Deform {
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// The same data for a mesh whose vertices were rearranged: vertex `i` of
    /// the new mesh is vertex `source[i]` of the old (what turning an indexed
    /// mesh into separate triangles does).
    pub fn picked(&self, source: &[u32]) -> Deform {
        let pick = |list: &Vec<[f32; 3]>| -> Vec<[f32; 3]> {
            if list.is_empty() {
                Vec::new()
            } else {
                source.iter().map(|&i| list[i as usize]).collect()
            }
        };
        Deform {
            positions: pick(&self.positions),
            normals: pick(&self.normals),
            skin: self.skin.as_ref().map(|skin| VertexSkin {
                joints: source.iter().map(|&i| skin.joints[i as usize]).collect(),
                weights: source.iter().map(|&i| skin.weights[i as usize]).collect(),
            }),
            targets: self
                .targets
                .iter()
                .map(|target| MorphTarget {
                    positions: pick(&target.positions),
                    normals: pick(&target.normals),
                })
                .collect(),
            faceted: self.faceted,
        }
    }

    /// Puts the positions and normals of `vertices` where `weights` (one for
    /// each morph target, the missing ones zero) and `joints` (the matrices of
    /// the mesh's skin) leave them. The texture coordinates and colours stay
    /// as they are. `vertices` is the mesh's own list: as long as this.
    pub fn apply(&self, joints: Option<&[Mat4]>, weights: &[f32], vertices: &mut [Vertex]) {
        debug_assert_eq!(vertices.len(), self.len());
        let threads = if vertices.len() >= THREADED_FROM {
            std::thread::available_parallelism().map_or(1, |n| n.get().min(8))
        } else {
            1
        };
        if threads <= 1 {
            self.apply_range(0, joints, weights, vertices);
        } else {
            let chunk = vertices.len().div_ceil(threads);
            std::thread::scope(|scope| {
                for (n, part) in vertices.chunks_mut(chunk).enumerate() {
                    scope.spawn(move || self.apply_range(n * chunk, joints, weights, part));
                }
            });
        }
        if self.faceted {
            crate::load::flat_normals_in_place(vertices);
        }
    }

    /// `apply` for the vertices from `first` on, as far as `part` goes.
    fn apply_range(
        &self,
        first: usize,
        joints: Option<&[Mat4]>,
        weights: &[f32],
        part: &mut [Vertex],
    ) {
        let skin = self.skin.as_ref().zip(joints);
        for (offset, vertex) in part.iter_mut().enumerate() {
            let i = first + offset;
            let mut position = Vec3::from(self.positions[i]);
            let mut normal = Vec3::from(self.normals[i]);
            for (target, &weight) in self.targets.iter().zip(weights) {
                if weight == 0.0 {
                    continue;
                }
                if let Some(delta) = target.positions.get(i) {
                    position += Vec3::from(*delta) * weight;
                }
                if let Some(delta) = target.normals.get(i) {
                    normal += Vec3::from(*delta) * weight;
                }
            }
            if let Some((skin, matrices)) = skin {
                // The blend of up to four joint matrices, weighted. A vertex
                // nothing weighs on stays where the targets left it.
                let mut blended = Mat4::ZERO;
                let mut total = 0.0;
                for (&joint, &weight) in skin.joints[i].iter().zip(&skin.weights[i]) {
                    if weight > 0.0
                        && let Some(matrix) = matrices.get(usize::from(joint))
                    {
                        blended += *matrix * weight;
                        total += weight;
                    }
                }
                if total > 1e-6 {
                    blended *= 1.0 / total;
                    position = blended.transform_point3(position);
                    normal = blended.transform_vector3(normal);
                }
            }
            vertex.position = position.to_array();
            vertex.normal = normal.try_normalize().unwrap_or(Vec3::Y).to_array();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(count: usize) -> Vec<Vertex> {
        vec![Vertex::at([0.0; 3]); count]
    }

    /// Three vertices along X, all with normal +Y, nothing acting on them.
    fn row() -> Deform {
        Deform {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            skin: None,
            targets: vec![],
            faceted: false,
        }
    }

    fn moved(deform: &Deform, joints: Option<&[Mat4]>, weights: &[f32]) -> Vec<Vertex> {
        let mut vertices = flat(deform.len());
        deform.apply(joints, weights, &mut vertices);
        vertices
    }

    #[test]
    fn with_nothing_acting_the_vertices_are_the_rest_pose() {
        let v = moved(&row(), None, &[]);
        assert_eq!(v[2].position, [2.0, 0.0, 0.0]);
        assert_eq!(v[2].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_morph_target_moves_vertices_in_proportion_to_its_weight() {
        let mut deform = row();
        deform.targets = vec![MorphTarget {
            positions: vec![[0.0, 2.0, 0.0]; 3],
            normals: vec![],
        }];
        assert_eq!(moved(&deform, None, &[0.0])[1].position, [1.0, 0.0, 0.0]);
        assert_eq!(moved(&deform, None, &[0.5])[1].position, [1.0, 1.0, 0.0]);
        assert_eq!(moved(&deform, None, &[1.0])[1].position, [1.0, 2.0, 0.0]);
        // A weight the mesh has no target for does nothing.
        assert_eq!(
            moved(&deform, None, &[0.0, 9.0])[1].position,
            [1.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a_target_that_tilts_the_normals_does_so_and_keeps_them_unit_length() {
        let mut deform = row();
        deform.targets = vec![MorphTarget {
            positions: vec![],
            normals: vec![[1.0, 0.0, 0.0]; 3],
        }];
        let n = moved(&deform, None, &[1.0])[0].normal;
        let length = n.iter().map(|c| c * c).sum::<f32>().sqrt();
        assert!((length - 1.0).abs() < 1e-5);
        assert!(n[0] > 0.5 && n[1] > 0.5, "{n:?}");
    }

    fn skinned() -> Deform {
        let mut deform = row();
        // Vertex 0 follows joint 0, vertex 2 joint 1, vertex 1 both equally.
        deform.skin = Some(VertexSkin {
            joints: vec![[0, 0, 0, 0], [0, 1, 0, 0], [1, 0, 0, 0]],
            weights: vec![
                [1.0, 0.0, 0.0, 0.0],
                [0.5, 0.5, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
            ],
        });
        deform
    }

    #[test]
    fn joints_carry_the_vertices_they_weigh_on() {
        let lift = Mat4::from_translation(Vec3::new(0.0, 4.0, 0.0));
        let v = moved(&skinned(), Some(&[Mat4::IDENTITY, lift]), &[]);
        assert_eq!(v[0].position, [0.0, 0.0, 0.0], "joint 0 stays");
        assert_eq!(v[1].position, [1.0, 2.0, 0.0], "half way between the two");
        assert_eq!(v[2].position, [2.0, 4.0, 0.0], "joint 1 lifts it");
    }

    #[test]
    fn a_turned_joint_turns_the_normals_too() {
        let turn = Mat4::from_rotation_z(-std::f32::consts::FRAC_PI_2);
        let v = moved(&skinned(), Some(&[turn, turn]), &[]);
        let n = v[0].normal;
        assert!((n[0] - 1.0).abs() < 1e-5 && n[1].abs() < 1e-5, "{n:?}");
    }

    #[test]
    fn a_vertex_no_joint_weighs_on_stays_where_the_targets_left_it() {
        let mut deform = skinned();
        deform.skin.as_mut().unwrap().weights[0] = [0.0; 4];
        let v = moved(&deform, Some(&[Mat4::from_scale(Vec3::splat(9.0)); 2]), &[]);
        assert_eq!(v[0].position, [0.0, 0.0, 0.0]);
        // A joint number past the end of the skin counts for nothing too.
        let mut deform = skinned();
        deform.skin.as_mut().unwrap().joints[2] = [7, 0, 0, 0];
        let v = moved(&deform, Some(&[Mat4::from_scale(Vec3::splat(9.0)); 2]), &[]);
        assert_eq!(v[2].position, [2.0, 0.0, 0.0]);
    }

    #[test]
    fn morphs_act_before_the_skin() {
        let mut deform = skinned();
        deform.targets = vec![MorphTarget {
            positions: vec![[0.0, 1.0, 0.0]; 3],
            normals: vec![],
        }];
        let scale = Mat4::from_scale(Vec3::splat(2.0));
        let v = moved(&deform, Some(&[scale, scale]), &[1.0]);
        // (1, 0, 0) + (0, 1, 0), then doubled.
        assert_eq!(v[1].position, [2.0, 2.0, 0.0]);
    }

    #[test]
    fn keeps_texture_coordinates_and_colours() {
        let mut deform = row();
        deform.targets = vec![MorphTarget {
            positions: vec![[1.0, 0.0, 0.0]; 3],
            normals: vec![],
        }];
        let mut vertices = flat(3);
        vertices[1].uv = [0.25, 0.75];
        vertices[1].color = [1, 2, 3, 4];
        deform.apply(None, &[1.0], &mut vertices);
        assert_eq!(vertices[1].uv, [0.25, 0.75]);
        assert_eq!(vertices[1].color, [1, 2, 3, 4]);
    }

    #[test]
    fn a_faceted_mesh_gets_the_normals_of_its_moved_faces() {
        // One triangle lying flat, facing up; a morph tips it onto its side.
        let deform = Deform {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            normals: vec![[0.0, 1.0, 0.0]; 3],
            skin: None,
            targets: vec![MorphTarget {
                positions: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, -1.0, 1.0]],
                normals: vec![],
            }],
            faceted: true,
        };
        let flat_up = moved(&deform, None, &[0.0]);
        assert!(flat_up[0].normal[1] > 0.99);
        let tipped = moved(&deform, None, &[1.0]);
        // The third corner went to (0, -1, 0): the face now looks along +Z.
        assert!(tipped[0].normal[2].abs() > 0.99, "{:?}", tipped[0].normal);
        assert_eq!(tipped[0].normal, tipped[2].normal);
    }

    #[test]
    fn picks_the_vertices_a_rearranged_mesh_kept() {
        let mut deform = skinned();
        deform.targets = vec![MorphTarget {
            positions: vec![[0.0, 1.0, 0.0], [0.0, 2.0, 0.0], [0.0, 3.0, 0.0]],
            normals: vec![],
        }];
        let picked = deform.picked(&[2, 2, 0, 1]);
        assert_eq!(picked.len(), 4);
        assert_eq!(picked.positions[0], [2.0, 0.0, 0.0]);
        assert_eq!(picked.positions[2], [0.0, 0.0, 0.0]);
        assert_eq!(picked.targets[0].positions[1], [0.0, 3.0, 0.0]);
        assert!(picked.targets[0].normals.is_empty(), "stays empty");
        assert_eq!(picked.skin.as_ref().unwrap().joints[0], [1, 0, 0, 0]);
        assert_eq!(picked.skin.as_ref().unwrap().weights[3][1], 0.5);
    }

    #[test]
    fn a_big_mesh_moved_on_several_threads_is_the_same_as_on_one() {
        let count = THREADED_FROM * 2 + 7;
        let positions: Vec<[f32; 3]> = (0..count).map(|i| [i as f32, 0.0, 0.0]).collect();
        let deform = Deform {
            normals: vec![[0.0, 1.0, 0.0]; count],
            skin: Some(VertexSkin {
                joints: vec![[0, 0, 0, 0]; count],
                weights: vec![[1.0, 0.0, 0.0, 0.0]; count],
            }),
            positions,
            targets: vec![],
            faceted: false,
        };
        let lift = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
        let v = moved(&deform, Some(&[lift]), &[]);
        for (i, vertex) in v.iter().enumerate() {
            assert_eq!(vertex.position, [i as f32, 1.0, 0.0], "vertex {i}");
        }
    }
}
