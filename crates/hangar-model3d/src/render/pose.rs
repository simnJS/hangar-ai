//! Putting an uploaded model in a pose: the matrix of every instance, and the
//! vertices of every mesh a clip deforms, written over what the GPU holds.
//!
//! A model is posed once for each time it is drawn at: a frame at the same
//! moment as the last (the camera moved, the clip is paused) costs nothing.
//! The GPU copy of a model is shared by every viewer that draws the same
//! `Model` on a device, and the last pose written before a frame is submitted
//! is the one all of them draw; the app opens one `Model` for each viewer, so
//! two viewers at different times of one model do not happen.

use super::upload::{GpuModel, PoseKey, normal_positions};
use super::{Gpu, LIVE_NORMALS_LIMIT};
use crate::animation::Playhead;
use crate::model::Model;

impl Gpu {
    /// Moves the instances and the deformed meshes of `uploaded` to where
    /// `at` puts them: the first frame of the clip it names, a later moment,
    /// or the file's own pose with `None` (or a clip the model does not
    /// have). `normals` says the normals' lines are shown, so they are moved
    /// too, up to a size where that costs too much.
    pub(super) fn pose(
        &mut self,
        model: &Model,
        uploaded: &mut GpuModel,
        at: Option<Playhead>,
        normals: bool,
    ) {
        let Some(rig) = &model.rig else { return };
        let playing = at.and_then(|head| model.clips.get(head.clip).map(|clip| (head, clip)));
        let key = PoseKey {
            at: playing.map(|(head, _)| (head.clip, head.time.to_bits())),
            normals,
        };
        if uploaded.posed == Some(key) {
            return;
        }
        let pose = rig.pose(playing.map(|(head, clip)| (clip, head.time)));

        // Where every instance stands, in one write.
        let align = self.object_align as usize;
        let mut objects = vec![0u8; align * model.instances.len().max(1)];
        for (index, instance) in model.instances.iter().enumerate() {
            let matrix = model.root * rig.instance_matrix(index, &pose);
            let offset = index * align;
            objects[offset..offset + 64]
                .copy_from_slice(bytemuck::cast_slice(&matrix.to_cols_array()));
            if let Some(standing) = uploaded.instances.get_mut(index) {
                standing.mirrored = matrix.determinant() < 0.0;
                let bounds = model.meshes[instance.mesh].bounds();
                standing.center = matrix
                    .transform_point3(glam::Vec3::from(bounds.center().map(|c| c as f32)))
                    .to_array();
            }
        }
        self.queue.write_buffer(&uploaded.objects, 0, &objects);

        // The vertices of the meshes that bend or morph.
        let mut scratch = std::mem::take(&mut self.scratch);
        for position in 0..uploaded.deformables.len() {
            let index = uploaded.deformables[position];
            let instance = &model.instances[index];
            let mesh = &model.meshes[instance.mesh];
            let (Some(deform), Some(attachment)) = (&mesh.deform, rig.attachments().get(index))
            else {
                continue;
            };
            let joints = attachment.skin.map(|skin| rig.joint_matrices(skin, &pose));
            scratch.clear();
            scratch.extend_from_slice(&mesh.vertices);
            deform.apply(joints.as_deref(), rig.weights(index, &pose), &mut scratch);
            let target = &mut uploaded.meshes[instance.mesh];
            self.queue
                .write_buffer(&target.vertices, 0, bytemuck::cast_slice(&scratch));
            if normals && scratch.len() <= LIVE_NORMALS_LIMIT {
                let lines = normal_positions(&scratch, target.local_radius * 0.02);
                match &target.normals {
                    Some(held) if held.count as usize == lines.len() => {
                        self.queue
                            .write_buffer(&held.buffer, 0, bytemuck::cast_slice(&lines));
                    }
                    _ => {
                        target.normals = self.normal_lines_of(&scratch, target.local_radius * 0.02);
                    }
                }
            }
        }
        self.scratch = scratch;
        uploaded.posed = Some(key);
    }
}
