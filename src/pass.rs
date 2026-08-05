//! The app's render pass: the wireframe ribbons and the glass shards.
//!
//! Both stages are drawn here rather than through the engine's material path
//! because both displace their geometry in the vertex stage. The shards fly in
//! from a scattered pose driven by their own centroid and seed, and the ribbons
//! expand into camera-facing quads, neither of which a mesh with a fixed vertex
//! layout can express. The third stage, the surface reveal itself, needs none
//! of that and rides the engine's reveal material instead.
//!
//! The pass draws into the scene color target after the opaque geometry, so it
//! composites over the revealed surface and its emission reaches bloom and
//! tonemapping with everything else.

use crate::frame::{GlassUniforms, Mailbox, WireUniforms};
use crate::geometry::{ShardVertex, WireVertex};
use nightshade::render::config::IblViews;
use nightshade::render::wgpu;
use nightshade::render::wgpu::render_configs::RenderInputs;
use nightshade::render::wgpu::rendergraph::{
    PassExecutionContext, PassNode, Result, SubGraphRunCommand,
};
use nightshade::render::wgpu::util::DeviceExt;

const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Premultiplied alpha over the scene color, so a bright leading edge adds
/// light instead of only tinting what is behind it.
const BLEND: wgpu::BlendState = wgpu::BlendState {
    color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
    alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
    },
};

/// Binds the renderer's image-based lighting to the glass pipeline, standing in
/// blank textures until the environment has been captured.
struct EnvironmentBinder {
    layout: wgpu::BindGroupLayout,
    environment_sampler: wgpu::Sampler,
    lut_sampler: wgpu::Sampler,
    fallback_cube: wgpu::TextureView,
    fallback_lut: wgpu::TextureView,
}

impl EnvironmentBinder {
    fn new(device: &wgpu::Device) -> Self {
        let cube_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::Cube,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Materialize Environment Layout"),
            entries: &[
                cube_entry(0),
                cube_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                sampler_entry(3),
                sampler_entry(4),
            ],
        });

        Self {
            layout,
            environment_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Materialize Environment Sampler"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            lut_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("Materialize Lut Sampler"),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            fallback_cube: blank_texture(device, 6, wgpu::TextureViewDimension::Cube),
            fallback_lut: blank_texture(device, 1, wgpu::TextureViewDimension::D2),
        }
    }

    fn bind_group(&self, device: &wgpu::Device, views: &IblViews) -> wgpu::BindGroup {
        let irradiance = views
            .irradiance_view
            .as_ref()
            .unwrap_or(&self.fallback_cube);
        let prefiltered = views
            .prefiltered_view
            .as_ref()
            .unwrap_or(&self.fallback_cube);
        let lut = views.brdf_lut_view.as_ref().unwrap_or(&self.fallback_lut);
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Materialize Environment Bind Group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(irradiance),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(prefiltered),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(lut),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.environment_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&self.lut_sampler),
                },
            ],
        })
    }
}

fn matrix_to_array(matrix: &nalgebra_glm::Mat4) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            result[column][row] = matrix[(row, column)];
        }
    }
    result
}

fn blank_texture(
    device: &wgpu::Device,
    layers: u32,
    dimension: wgpu::TextureViewDimension,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("Materialize Blank Texture"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(dimension),
        ..Default::default()
    })
}

/// One uploaded mesh: static geometry plus the uniform buffers the systems
/// refresh each frame.
struct MeshBuffers {
    wire_vertices: wgpu::Buffer,
    wire_indices: wgpu::Buffer,
    wire_index_count: u32,
    wire_uniforms: wgpu::Buffer,
    wire_bind_group: wgpu::BindGroup,
    shards: wgpu::Buffer,
    shard_vertex_count: u32,
    glass_uniforms: wgpu::Buffer,
    glass_bind_group: wgpu::BindGroup,
}

pub struct MaterializePass {
    mailbox: Mailbox,
    wire_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
    uniform_layout: wgpu::BindGroupLayout,
    environment: EnvironmentBinder,
    environment_bind_group: wgpu::BindGroup,
    meshes: Vec<MeshBuffers>,
}

impl MaterializePass {
    pub fn new(device: &wgpu::Device, mailbox: Mailbox) -> Self {
        let wire_shader = wgpu::shader_compose::compile_wgsl(
            device,
            "materialize_wire.wgsl",
            include_str!("shaders/materialize_wire.wgsl"),
        );
        let glass_shader = wgpu::shader_compose::compile_wgsl(
            device,
            "materialize_glass.wgsl",
            include_str!("shaders/materialize_glass.wgsl"),
        );

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Materialize Uniform Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let environment = EnvironmentBinder::new(device);
        let environment_bind_group = environment.bind_group(device, &IblViews::default());

        let wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Materialize Wire Pipeline Layout"),
            bind_group_layouts: &[Some(&uniform_layout)],
            immediate_size: 0,
        });
        let glass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Materialize Glass Pipeline Layout"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&environment.layout)],
            immediate_size: 0,
        });

        let attributes = wgpu::vertex_attr_array![
            0 => Float32x3,
            1 => Float32x3,
            2 => Float32x3,
            3 => Float32,
        ];

        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        shader: &wgpu::ShaderModule,
                        stride: u64| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &attributes,
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        blend: Some(BLEND),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };

        let wire_pipeline = pipeline(
            "Materialize Wire Pipeline",
            &wire_layout,
            &wire_shader,
            std::mem::size_of::<WireVertex>() as u64,
        );
        let glass_pipeline = pipeline(
            "Materialize Glass Pipeline",
            &glass_layout,
            &glass_shader,
            std::mem::size_of::<ShardVertex>() as u64,
        );

        Self {
            mailbox,
            wire_pipeline,
            glass_pipeline,
            uniform_layout,
            environment,
            environment_bind_group,
            meshes: Vec::new(),
        }
    }
}

impl PassNode<RenderInputs> for MaterializePass {
    fn name(&self) -> &str {
        "materialize_pass"
    }

    fn reads(&self) -> Vec<&str> {
        vec![]
    }

    fn writes(&self) -> Vec<&str> {
        vec![]
    }

    fn reads_writes(&self) -> Vec<&str> {
        vec!["color", "depth"]
    }

    fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, configs: &RenderInputs) {
        let Ok(mut frame) = self.mailbox.lock() else {
            return;
        };

        if let Some(pending) = frame.pending_geometry.take() {
            self.meshes = pending
                .into_iter()
                .map(|geometry| {
                    let uniform_buffer = |label: &str, size: u64| {
                        device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some(label),
                            size,
                            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        })
                    };
                    let bind_group = |buffer: &wgpu::Buffer, label: &str| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some(label),
                            layout: &self.uniform_layout,
                            entries: &[wgpu::BindGroupEntry {
                                binding: 0,
                                resource: buffer.as_entire_binding(),
                            }],
                        })
                    };

                    let wire_uniforms = uniform_buffer(
                        "Materialize Wire Uniforms",
                        std::mem::size_of::<WireUniforms>() as u64,
                    );
                    let glass_uniforms = uniform_buffer(
                        "Materialize Glass Uniforms",
                        std::mem::size_of::<GlassUniforms>() as u64,
                    );

                    MeshBuffers {
                        wire_vertices: device.create_buffer_init(
                            &wgpu::util::BufferInitDescriptor {
                                label: Some("Materialize Wire Vertices"),
                                contents: bytemuck::cast_slice(&geometry.wire_vertices),
                                usage: wgpu::BufferUsages::VERTEX,
                            },
                        ),
                        wire_indices: device.create_buffer_init(
                            &wgpu::util::BufferInitDescriptor {
                                label: Some("Materialize Wire Indices"),
                                contents: bytemuck::cast_slice(&geometry.wire_indices),
                                usage: wgpu::BufferUsages::INDEX,
                            },
                        ),
                        wire_index_count: geometry.wire_indices.len() as u32,
                        wire_bind_group: bind_group(&wire_uniforms, "Materialize Wire Bind Group"),
                        wire_uniforms,
                        shards: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("Materialize Shards"),
                            contents: bytemuck::cast_slice(&geometry.shards),
                            usage: wgpu::BufferUsages::VERTEX,
                        }),
                        shard_vertex_count: geometry.shards.len() as u32,
                        glass_bind_group: bind_group(
                            &glass_uniforms,
                            "Materialize Glass Bind Group",
                        ),
                        glass_uniforms,
                    }
                })
                .collect();
        }

        if let Some(view) = configs.scene.render_view.as_ref() {
            let view_projection = matrix_to_array(&(view.projection * view.view));
            let inverse_view = nalgebra_glm::inverse(&view.view);
            let camera_position = [
                inverse_view[(0, 3)],
                inverse_view[(1, 3)],
                inverse_view[(2, 3)],
                1.0,
            ];
            for (buffers, draw) in self.meshes.iter().zip(frame.draws.iter()) {
                let mut wire = draw.wire;
                wire.view_projection = view_projection;
                wire.camera_position = camera_position;
                let mut glass = draw.glass;
                glass.view_projection = view_projection;
                glass.camera_position = camera_position;
                queue.write_buffer(&buffers.wire_uniforms, 0, bytemuck::bytes_of(&wire));
                queue.write_buffer(&buffers.glass_uniforms, 0, bytemuck::bytes_of(&glass));
            }
        }

        self.environment_bind_group = self.environment.bind_group(device, &configs.ibl_views);
    }

    fn execute<'r, 'e>(
        &mut self,
        context: PassExecutionContext<'r, 'e, RenderInputs>,
    ) -> Result<Vec<SubGraphRunCommand<'r>>> {
        if self.meshes.is_empty() {
            return Ok(context.into_sub_graph_commands());
        }

        let (color_view, color_load, color_store) = context.get_color_attachment("color")?;
        let (depth_view, depth_load, depth_store) = context.get_depth_attachment("depth")?;

        {
            let mut render_pass = context
                .encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("Materialize Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: color_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: color_load,
                            store: color_store,
                        },
                        depth_slice: None,
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: depth_load,
                            store: depth_store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });

            render_pass.set_pipeline(&self.wire_pipeline);
            for buffers in &self.meshes {
                render_pass.set_bind_group(0, &buffers.wire_bind_group, &[]);
                render_pass.set_vertex_buffer(0, buffers.wire_vertices.slice(..));
                render_pass
                    .set_index_buffer(buffers.wire_indices.slice(..), wgpu::IndexFormat::Uint32);
                render_pass.draw_indexed(0..buffers.wire_index_count, 0, 0..1);
            }

            render_pass.set_pipeline(&self.glass_pipeline);
            render_pass.set_bind_group(1, &self.environment_bind_group, &[]);
            for buffers in &self.meshes {
                render_pass.set_bind_group(0, &buffers.glass_bind_group, &[]);
                render_pass.set_vertex_buffer(0, buffers.shards.slice(..));
                render_pass.draw(0..buffers.shard_vertex_count, 0..1);
            }
        }

        Ok(context.into_sub_graph_commands())
    }
}
