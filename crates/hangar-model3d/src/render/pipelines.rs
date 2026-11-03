//! The bind group layouts and the render pipelines. All of them draw into the
//! same multisampled colour and depth targets, so none depends on the format
//! of the window the picture ends up in.

use std::borrow::Cow;
use std::collections::HashMap;
use std::num::NonZeroU64;

use iced::wgpu;

use super::{COLOR_FORMAT, DEPTH_FORMAT, SAMPLES};
use crate::model::Vertex;

const COMMON: &str = include_str!("shaders/common.wgsl");
const MESH: &str = include_str!("shaders/mesh.wgsl");
const OVERLAY: &str = include_str!("shaders/overlay.wgsl");
const GRID: &str = include_str!("shaders/grid.wgsl");
const MIP: &str = include_str!("shaders/mip.wgsl");

/// What a mesh pipeline draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Kind {
    Triangles,
    Lines,
    Points,
}

/// Which faces are thrown away. A mirrored instance (a negative scale) turns
/// its faces around, and needs the other winding to be seen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Cull {
    Back,
    BackMirrored,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct MeshKey {
    pub kind: Kind,
    pub blend: bool,
    pub cull: Cull,
}

pub(super) struct Layouts {
    /// The scene: the camera, and the object's matrix at a dynamic offset.
    pub frame: wgpu::BindGroupLayout,
    pub material: wgpu::BindGroupLayout,
    pub style: wgpu::BindGroupLayout,
    pub grid: wgpu::BindGroupLayout,
    pub mip: wgpu::BindGroupLayout,
}

pub(super) struct Pipelines {
    pub mesh: HashMap<MeshKey, wgpu::RenderPipeline>,
    pub wire: wgpu::RenderPipeline,
    pub normals: wgpu::RenderPipeline,
    pub axes: wgpu::RenderPipeline,
    pub grid: wgpu::RenderPipeline,
    pub mip: wgpu::RenderPipeline,
    pub mip_srgb: wgpu::RenderPipeline,
}

fn uniform(
    binding: u32,
    visibility: wgpu::ShaderStages,
    dynamic: Option<u64>,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic.is_some(),
            min_binding_size: dynamic.and_then(NonZeroU64::new),
        },
        count: None,
    }
}

fn texture(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn sampler(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

impl Layouts {
    pub fn new(device: &wgpu::Device) -> Layouts {
        let both = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let make = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let mut material = vec![uniform(0, wgpu::ShaderStages::FRAGMENT, None)];
        material.extend((1..=5).map(texture));
        material.extend((6..=10).map(sampler));
        Layouts {
            frame: make(
                "model3d frame",
                &[
                    uniform(0, both, None),
                    uniform(1, wgpu::ShaderStages::VERTEX, Some(64)),
                ],
            ),
            material: make("model3d material", &material),
            style: make(
                "model3d style",
                &[uniform(0, wgpu::ShaderStages::FRAGMENT, Some(16))],
            ),
            grid: make("model3d grid", &[uniform(0, both, None)]),
            mip: make("model3d mip", &[texture(0), sampler(1)]),
        }
    }
}

fn module(device: &wgpu::Device, label: &str, parts: &[&str]) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(parts.join("\n"))),
    })
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
    0 => Float32x3,
    1 => Float32x3,
    2 => Float32x2,
    3 => Unorm8x4,
];

const POSITION_ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32x3];

fn depth(
    write: bool,
    compare: wgpu::CompareFunction,
    bias: wgpu::DepthBiasState,
) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: write,
        depth_compare: compare,
        stencil: wgpu::StencilState::default(),
        bias,
    }
}

fn target(blend: Option<wgpu::BlendState>) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState {
        format: COLOR_FORMAT,
        blend,
        write_mask: wgpu::ColorWrites::ALL,
    })
}

fn multisample() -> wgpu::MultisampleState {
    wgpu::MultisampleState {
        count: SAMPLES,
        mask: !0,
        alpha_to_coverage_enabled: false,
    }
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, layouts: &Layouts) -> Pipelines {
        let layout = |label: &str, groups: &[&wgpu::BindGroupLayout]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                push_constant_ranges: &[],
            })
        };
        let mesh_layout = layout("model3d mesh", &[&layouts.frame, &layouts.material]);
        let overlay_layout = layout("model3d overlay", &[&layouts.frame, &layouts.style]);
        let grid_layout = layout("model3d grid", &[&layouts.frame, &layouts.grid]);
        let mip_layout = layout("model3d mip", &[&layouts.mip]);

        let mesh_module = module(device, "model3d mesh", &[COMMON, MESH]);
        let overlay_module = module(device, "model3d overlay", &[COMMON, OVERLAY]);
        let grid_module = module(device, "model3d grid", &[COMMON, GRID]);
        let mip_module = module(device, "model3d mip", &[MIP]);

        let mut mesh = HashMap::new();
        let kinds = [Kind::Triangles, Kind::Lines, Kind::Points];
        for kind in kinds {
            let culls: &[Cull] = if kind == Kind::Triangles {
                &[Cull::Back, Cull::BackMirrored, Cull::Nothing]
            } else {
                &[Cull::Nothing]
            };
            let blends: &[bool] = if kind == Kind::Triangles {
                &[false, true]
            } else {
                &[false]
            };
            for &cull in culls {
                for &blend in blends {
                    let key = MeshKey { kind, blend, cull };
                    let (vertex_entry, fragment_entry, topology, step) = match kind {
                        Kind::Triangles => (
                            "vs_main",
                            "fs_main",
                            wgpu::PrimitiveTopology::TriangleList,
                            wgpu::VertexStepMode::Vertex,
                        ),
                        Kind::Lines => (
                            "vs_main",
                            "fs_unlit",
                            wgpu::PrimitiveTopology::LineList,
                            wgpu::VertexStepMode::Vertex,
                        ),
                        Kind::Points => (
                            "vs_points",
                            "fs_unlit",
                            wgpu::PrimitiveTopology::TriangleList,
                            wgpu::VertexStepMode::Instance,
                        ),
                    };
                    // Lines and points may carry an alpha of their own; the
                    // triangles blend only when their material asks.
                    let blending = (blend || kind != Kind::Triangles)
                        .then_some(wgpu::BlendState::ALPHA_BLENDING);
                    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("model3d mesh"),
                        layout: Some(&mesh_layout),
                        vertex: wgpu::VertexState {
                            module: &mesh_module,
                            entry_point: Some(vertex_entry),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            buffers: &[wgpu::VertexBufferLayout {
                                array_stride: std::mem::size_of::<Vertex>() as u64,
                                step_mode: step,
                                attributes: &VERTEX_ATTRIBUTES,
                            }],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &mesh_module,
                            entry_point: Some(fragment_entry),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            targets: &[target(blending)],
                        }),
                        primitive: wgpu::PrimitiveState {
                            topology,
                            front_face: if cull == Cull::BackMirrored {
                                wgpu::FrontFace::Cw
                            } else {
                                wgpu::FrontFace::Ccw
                            },
                            cull_mode: (cull != Cull::Nothing).then_some(wgpu::Face::Back),
                            ..wgpu::PrimitiveState::default()
                        },
                        depth_stencil: Some(depth(
                            !blend,
                            wgpu::CompareFunction::LessEqual,
                            wgpu::DepthBiasState::default(),
                        )),
                        multisample: multisample(),
                        multiview: None,
                        cache: None,
                    });
                    mesh.insert(key, pipeline);
                }
            }
        }

        let overlay = |label: &str,
                       stride: u64,
                       attributes: &[wgpu::VertexAttribute],
                       depth_state: wgpu::DepthStencilState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&overlay_layout),
                vertex: wgpu::VertexState {
                    module: &overlay_module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &overlay_module,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[target(Some(wgpu::BlendState::ALPHA_BLENDING))],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    ..wgpu::PrimitiveState::default()
                },
                depth_stencil: Some(depth_state),
                multisample: multisample(),
                multiview: None,
                cache: None,
            })
        };
        // Drawn a hair toward the camera, so a wire along a face's edge wins
        // the depth test against the face it lies on.
        let toward_camera = wgpu::DepthBiasState {
            constant: -4,
            slope_scale: 0.0,
            clamp: 0.0,
        };
        let wire = overlay(
            "model3d wire",
            std::mem::size_of::<Vertex>() as u64,
            &VERTEX_ATTRIBUTES[..1],
            depth(false, wgpu::CompareFunction::LessEqual, toward_camera),
        );
        let normals = overlay(
            "model3d normals",
            12,
            &POSITION_ATTRIBUTES,
            depth(false, wgpu::CompareFunction::LessEqual, toward_camera),
        );
        // Over the model, wherever the origin is: it matters most when the
        // origin is inside the mesh.
        let axes = overlay(
            "model3d axes",
            12,
            &POSITION_ATTRIBUTES,
            depth(
                false,
                wgpu::CompareFunction::Always,
                wgpu::DepthBiasState::default(),
            ),
        );

        let grid = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("model3d grid"),
            layout: Some(&grid_layout),
            vertex: wgpu::VertexState {
                module: &grid_module,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &grid_module,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[target(Some(wgpu::BlendState::ALPHA_BLENDING))],
            }),
            primitive: wgpu::PrimitiveState::default(),
            // Pushed back in depth, so a model whose underside sits exactly
            // on the floor — most of them — wins the depth test: no
            // z-fighting from any side.
            depth_stencil: Some(depth(
                false,
                wgpu::CompareFunction::LessEqual,
                wgpu::DepthBiasState {
                    constant: 4,
                    slope_scale: 1.0,
                    clamp: 0.0,
                },
            )),
            multisample: multisample(),
            multiview: None,
            cache: None,
        });

        let mip = |format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("model3d mip"),
                layout: Some(&mip_layout),
                vertex: wgpu::VertexState {
                    module: &mip_module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mip_module,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        Pipelines {
            mesh,
            wire,
            normals,
            axes,
            grid,
            mip: mip(wgpu::TextureFormat::Rgba8Unorm),
            mip_srgb: mip(wgpu::TextureFormat::Rgba8UnormSrgb),
        }
    }
}
