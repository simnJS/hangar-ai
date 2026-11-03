//! What a loaded model is made of, as the agent that showed it gets it back
//! and the info strip lists it.
//!
//! Counts are what the GPU draws: a mesh several nodes stand on counts its
//! triangles and vertices once for each, and a range that leaves part of a
//! buffer out leaves it out of the count too.

use std::collections::BTreeSet;

use glam::Mat4;

use crate::model::{Clip, Model, Topology};

/// Above this many vertices the bounds come from each mesh's box, not its
/// vertices.
pub const PRECISE_BOUNDS_LIMIT: usize = 2_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ModelCounts {
    pub triangles: usize,
    pub vertices: usize,
    pub meshes: usize,
    pub materials: usize,
}

/// What `model_show` reports and the info strip lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelInfo {
    pub triangles: usize,
    pub vertices: usize,
    pub meshes: usize,
    pub materials: usize,
    /// Animation clip names, in file order.
    pub animations: Vec<String>,
    /// Bounding box size and centre, in the file's units.
    pub size: [f64; 3],
    pub center: [f64; 3],
}

/// The material a range draws with. A range without one is drawn in the
/// default material, which counts as a material of its own.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum MaterialKey {
    Index(usize),
    Default,
}

pub fn count_geometry(model: &Model) -> ModelCounts {
    let mut counts = ModelCounts::default();
    let mut materials = BTreeSet::new();
    for instance in &model.instances {
        let mesh = &model.meshes[instance.mesh];
        counts.vertices += mesh.vertices.len();
        if mesh.topology == Topology::Triangles {
            counts.meshes += 1;
            counts.triangles += mesh.drawn_elements() / 3;
        }
        for range in &mesh.ranges {
            materials.insert(match range.material {
                Some(index) => MaterialKey::Index(index),
                None => MaterialKey::Default,
            });
        }
    }
    counts.materials = materials.len();
    counts
}

/// Clip names in file order; an unnamed clip is named by its place.
pub fn clip_names(clips: &[Clip]) -> Vec<String> {
    clips
        .iter()
        .enumerate()
        .map(|(i, clip)| {
            let name = clip.name.trim();
            if name.is_empty() {
                format!("Animation {}", i + 1)
            } else {
                name.to_string()
            }
        })
        .collect()
}

/// A number to six significant digits, as `Number(value.toPrecision(6))` was.
fn round_significant(value: f64) -> f64 {
    format!("{value:.5e}").parse().unwrap_or(value)
}

/// Counts, size and centre of a model in the file's own coordinates.
pub fn model_info(model: &Model) -> ModelInfo {
    let counts = count_geometry(model);
    let bounds = model.bounds_under(Mat4::IDENTITY);
    let empty = !model
        .instances
        .iter()
        .any(|i| !model.meshes[i.mesh].vertices.is_empty());
    let round = |value: f64| if empty { 0.0 } else { round_significant(value) };
    let size = bounds.size();
    let center = bounds.center();
    ModelInfo {
        triangles: counts.triangles,
        vertices: counts.vertices,
        meshes: counts.meshes,
        materials: counts.materials,
        animations: clip_names(&model.clips),
        size: size.map(round),
        center: center.map(round),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::ModelFormat;
    use crate::model::testing::{cube_mesh, standing};
    use crate::model::{DrawRange, Material, Mesh, ModelParts, Vertex};

    fn model(
        meshes: Vec<Mesh>,
        instances: Vec<crate::model::Instance>,
        n_materials: usize,
    ) -> Model {
        Model::new(
            "test",
            ModelFormat::Glb,
            ModelParts {
                meshes,
                instances,
                materials: vec![Material::default(); n_materials],
                ..ModelParts::default()
            },
        )
    }

    fn single(mesh: Mesh) -> Model {
        model(vec![mesh], vec![standing(0, Mat4::IDENTITY)], 1)
    }

    #[test]
    fn counts_an_indexed_box_as_twelve_triangles_over_twenty_four_vertices() {
        let counts = count_geometry(&single(cube_mesh(Some(0))));
        assert_eq!(
            counts,
            ModelCounts {
                triangles: 12,
                vertices: 24,
                meshes: 1,
                materials: 1
            }
        );
    }

    #[test]
    fn counts_a_non_indexed_geometry_by_its_vertices() {
        let indexed = cube_mesh(Some(0));
        let vertices: Vec<Vertex> = indexed
            .indices
            .as_ref()
            .unwrap()
            .iter()
            .map(|&i| indexed.vertices[i as usize])
            .collect();
        let flat = Mesh::new("cube", Topology::Triangles, vertices, None, Some(0));
        let counts = count_geometry(&single(flat));
        assert_eq!(counts.triangles, 12);
        assert_eq!(counts.vertices, 36);
    }

    #[test]
    fn counts_every_placement_of_a_shared_mesh() {
        let instances = (0..5)
            .map(|i| standing(0, Mat4::from_translation([i as f32, 0.0, 0.0].into())))
            .collect();
        let counts = count_geometry(&model(vec![cube_mesh(Some(0))], instances, 1));
        assert_eq!(counts.triangles, 60);
        assert_eq!(counts.vertices, 120);
        assert_eq!(counts.meshes, 5);
        assert_eq!(counts.materials, 1);
    }

    #[test]
    fn leaves_out_what_the_draw_range_leaves_out() {
        let mut mesh = cube_mesh(Some(0));
        mesh.ranges = vec![DrawRange {
            start: 6,
            count: 12,
            material: Some(0),
        }];
        assert_eq!(count_geometry(&single(mesh)).triangles, 4);
    }

    #[test]
    fn holds_a_range_to_the_end_of_the_buffer() {
        let mut mesh = cube_mesh(Some(0));
        mesh.ranges = vec![DrawRange {
            start: 30,
            count: 999,
            material: Some(0),
        }];
        assert_eq!(count_geometry(&single(mesh)).triangles, 2);
    }

    #[test]
    fn counts_a_shared_material_once() {
        let mut two = cube_mesh(Some(0));
        two.ranges = vec![
            DrawRange {
                start: 0,
                count: 18,
                material: Some(0),
            },
            DrawRange {
                start: 18,
                count: 18,
                material: Some(1),
            },
        ];
        let meshes = vec![cube_mesh(Some(0)), cube_mesh(Some(0)), two];
        let instances = (0..3).map(|i| standing(i, Mat4::IDENTITY)).collect();
        let counts = count_geometry(&model(meshes, instances, 2));
        assert_eq!(
            (counts.meshes, counts.materials, counts.triangles),
            (3, 2, 36)
        );
    }

    #[test]
    fn counts_the_default_material_as_one_of_its_own() {
        let counts = count_geometry(&single(cube_mesh(None)));
        assert_eq!(counts.materials, 1);
    }

    #[test]
    fn counts_the_vertices_of_point_clouds_and_lines_but_no_triangles() {
        let cloud = Mesh::new(
            "cloud",
            Topology::Points,
            vec![Vertex::at([0.0; 3]); 100],
            None,
            None,
        );
        let lines = cube_mesh(None);
        let lines = Mesh::new(
            "lines",
            Topology::Lines,
            lines.vertices,
            lines.indices,
            None,
        );
        let counts = count_geometry(&model(
            vec![cloud, lines],
            vec![standing(0, Mat4::IDENTITY), standing(1, Mat4::IDENTITY)],
            0,
        ));
        assert_eq!(counts.triangles, 0);
        assert_eq!(counts.vertices, 124);
        assert_eq!(counts.meshes, 0);
    }

    #[test]
    fn measures_the_model_where_it_stands_scale_included() {
        let transform = Mat4::from_translation([2.0, 0.5, 0.0].into())
            * Mat4::from_scale([4.0, 1.0, 2.0].into());
        let info = single_at(transform).info;
        assert_eq!(info.size, [4.0, 1.0, 2.0]);
        assert_eq!(info.center, [2.0, 0.5, 0.0]);
    }

    fn single_at(transform: Mat4) -> Model {
        model(vec![cube_mesh(None)], vec![standing(0, transform)], 0)
    }

    #[test]
    fn measures_in_the_files_coordinates_not_the_upright_display() {
        // An STL is Z-up: its height is along Z in the file, and that is the
        // size the agent is told even though the viewer stands it up.
        let slab = Mat4::from_scale([1.0, 2.0, 5.0].into());
        let model = Model::new(
            "part.stl",
            ModelFormat::Stl,
            ModelParts {
                meshes: vec![cube_mesh(None)],
                instances: vec![standing(0, slab)],
                ..ModelParts::default()
            },
        );
        assert_eq!(model.info.size, [1.0, 2.0, 5.0]);
        // Displayed, the 5 is the height.
        let shown = model.bounds.size();
        assert!((shown[1] - 5.0).abs() < 1e-5, "{shown:?}");
    }

    #[test]
    fn reports_an_empty_model_as_nothing_at_the_origin() {
        let info = model(vec![], vec![], 0).info;
        assert_eq!(info.triangles, 0);
        assert_eq!(info.size, [0.0; 3]);
        assert_eq!(info.center, [0.0; 3]);
        assert!(info.animations.is_empty());
    }

    #[test]
    fn rounds_to_six_significant_digits() {
        assert_eq!(round_significant(1.0 / 3.0), 0.333333);
        assert_eq!(round_significant(1234567.0), 1234570.0);
        assert_eq!(round_significant(0.0), 0.0);
    }

    #[test]
    fn lists_the_clips_in_file_order_naming_the_unnamed_ones() {
        let clips = [
            Clip {
                name: "Flight".into(),
                duration: 1.0,
            },
            Clip {
                name: "  ".into(),
                duration: 1.0,
            },
        ];
        assert_eq!(clip_names(&clips), ["Flight", "Animation 2"]);
    }

    #[test]
    fn frames_points_on_the_model_not_its_box() {
        let m = single_at(Mat4::IDENTITY);
        assert_eq!(m.hull.len(), 24);
        assert!((m.radius - (3f64.sqrt() / 2.0)).abs() < 1e-6);
    }

    #[test]
    fn ignores_vertices_that_are_not_numbers() {
        let vertices = vec![
            Vertex::at([0.0; 3]),
            Vertex::at([1.0, 1.0, 1.0]),
            Vertex::at([f32::NAN, 0.0, 0.0]),
            Vertex::at([f32::INFINITY, 0.0, 0.0]),
        ];
        let cloud = Mesh::new("bad", Topology::Points, vertices, None, None);
        let m = model(vec![cloud], vec![standing(0, Mat4::IDENTITY)], 0);
        assert_eq!(m.info.size, [1.0; 3]);
        assert!(m.hull.iter().all(|p| p.iter().all(|c| c.is_finite())));
        assert!(m.radius.is_finite() && m.radius > 0.0);
    }

    #[test]
    fn samples_a_big_model_down_to_a_few_thousand_points() {
        let vertices = (0..50_000)
            .map(|i| Vertex::at([i as f32, 0.0, 0.0]))
            .collect();
        let cloud = Mesh::new("cloud", Topology::Points, vertices, None, None);
        let m = model(vec![cloud], vec![standing(0, Mat4::IDENTITY)], 0);
        assert!(m.hull.len() <= 6000, "{}", m.hull.len());
        assert!(m.hull.len() > 3000);
    }
}
