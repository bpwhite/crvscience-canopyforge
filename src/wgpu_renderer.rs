// Copyright (C) 2026 CRV Science Network (https://crvscience.com)
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://gnu.org>.

#![forbid(unsafe_code)]

use crate::models_3d::{SceneMeshBuffers, Vertex3D};
use eframe::{egui, egui_wgpu, wgpu};
use glam::{Mat4, Vec3, Vec4};
use std::borrow::Cow;
use std::sync::Arc;

const SHADOW_MAP_DIM: u32 = 2048;
const MSAA_SAMPLES: u32 = 4;

#[derive(Clone)]
pub struct SceneFrameUniforms {
    pub view_proj: Mat4,
    pub light_view_proj: Mat4,
    pub camera_pos: Vec4,
    pub sun_dir_and_time: Vec4,
    pub sun_color_and_vpd: Vec4,
    pub fog_color_and_density: Vec4,
}

impl SceneFrameUniforms {
    pub fn to_le_bytes(&self) -> [u8; 192] {
        let mut out = [0_u8; 192];
        let mut cursor = 0_usize;
        let mut push_floats = |slice: &[f32]| {
            for &f in slice {
                out[cursor..cursor + 4].copy_from_slice(&f.to_le_bytes());
                cursor += 4;
            }
        };
        push_floats(&self.view_proj.to_cols_array());
        push_floats(&self.light_view_proj.to_cols_array());
        push_floats(&self.camera_pos.to_array());
        push_floats(&self.sun_dir_and_time.to_array());
        push_floats(&self.sun_color_and_vpd.to_array());
        push_floats(&self.fog_color_and_density.to_array());
        out
    }
}

pub struct WatershedGpuResources {
    shadow_pipeline: wgpu::RenderPipeline,
    opaque_pipeline: wgpu::RenderPipeline,
    alpha_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,
    scene_uniform_buf: wgpu::Buffer,
    shadow_bind_group: wgpu::BindGroup,
    scene_bind_group: wgpu::BindGroup,
    tonemap_bgl: wgpu::BindGroupLayout,
    tonemap_sampler: wgpu::Sampler,
    tonemap_uniform_buf: wgpu::Buffer,
    shadow_view: wgpu::TextureView,
    vertex_buf: wgpu::Buffer,
    vertex_capacity_bytes: u64,
    target_size: (u32, u32),
    msaa_view: wgpu::TextureView,
    hdr_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    tonemap_bind_group: wgpu::BindGroup,
}

impl WatershedGpuResources {
    pub fn new(device: &wgpu::Device, swapchain_format: wgpu::TextureFormat) -> Self {
        let pbr_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canopyforge_watershed_pbr"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "shaders/watershed_pbr.wgsl"
            ))),
        });
        let tonemap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("canopyforge_post_tonemap"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "shaders/post_tonemap.wgsl"
            ))),
        });

        let scene_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scene_uniform_buf"),
            size: 192,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("directional_shadow_map"),
            size: wgpu::Extent3d {
                width: SHADOW_MAP_DIM,
                height: SHADOW_MAP_DIM,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let shadow_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_cmp_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        let shadow_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_bg"),
            layout: &shadow_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: scene_uniform_buf.as_entire_binding(),
            }],
        });

        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shadow_pipeline_layout"),
                bind_group_layouts: &[&shadow_bgl],
                push_constant_ranges: &[],
            });

        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });

        let scene_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scene_bg"),
            layout: &scene_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&shadow_sampler),
                },
            ],
        });

        let scene_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("scene_pipeline_layout"),
                bind_group_layouts: &[&scene_bgl],
                push_constant_ranges: &[],
            });

        let vbuf_attrs = [
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 0,
                shader_location: 0,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x3,
                offset: 12,
                shader_location: 1,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 24,
                shader_location: 2,
            },
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 40,
                shader_location: 3,
            },
        ];
        let vbuf_layout = wgpu::VertexBufferLayout {
            array_stride: Vertex3D::BYTE_STRIDE,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &vbuf_attrs,
        };

        let shadow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow_pipeline"),
            layout: Some(&shadow_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &pbr_shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vbuf_layout),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: None,
            multiview: None,
            cache: None,
        });

        let hdr_format = wgpu::TextureFormat::Rgba16Float;

        let opaque_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("opaque_pbr_pipeline"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &pbr_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vbuf_layout),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &pbr_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let alpha_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("alpha_pbr_pipeline"),
            layout: Some(&scene_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &pbr_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: std::slice::from_ref(&vbuf_layout),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: MSAA_SAMPLES,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            fragment: Some(wgpu::FragmentState {
                module: &pbr_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let tonemap_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let tonemap_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tonemap_layout"),
            bind_group_layouts: &[&tonemap_bgl],
            push_constant_ranges: &[],
        });

        let tonemap_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap_pipeline"),
            layout: Some(&tonemap_layout),
            vertex: wgpu::VertexState {
                module: &tonemap_shader,
                entry_point: Some("vs_tonemap"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &tonemap_shader,
                entry_point: Some("fs_tonemap"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: swapchain_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });

        let tonemap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("tonemap_linear_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let apply_gamma: f32 = if swapchain_format.is_srgb() { 0.0 } else { 1.0 };
        let tonemap_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tonemap_params_buf"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        {
            let mut mapped = tonemap_uniform_buf.slice(..).get_mapped_range_mut();
            mapped[0..4].copy_from_slice(&apply_gamma.to_le_bytes());
            mapped[4..16].fill(0);
        }
        tonemap_uniform_buf.unmap();

        let initial_cap = 4 * 1024 * 1024_u64;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("watershed_dynamic_vbuf"),
            size: initial_cap,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (msaa_view, hdr_view, depth_view, tonemap_bind_group) = Self::create_offscreen_targets(
            device,
            &tonemap_bgl,
            &tonemap_sampler,
            &tonemap_uniform_buf,
            1280,
            720,
        );

        Self {
            shadow_pipeline,
            opaque_pipeline,
            alpha_pipeline,
            tonemap_pipeline,
            scene_uniform_buf,
            shadow_bind_group,
            scene_bind_group,
            tonemap_bgl,
            tonemap_sampler,
            tonemap_uniform_buf,
            shadow_view,
            vertex_buf,
            vertex_capacity_bytes: initial_cap,
            target_size: (1280, 720),
            msaa_view,
            hdr_view,
            depth_view,
            tonemap_bind_group,
        }
    }

    fn create_offscreen_targets(
        device: &wgpu::Device,
        tonemap_bgl: &wgpu::BindGroupLayout,
        tonemap_sampler: &wgpu::Sampler,
        tonemap_uniform_buf: &wgpu::Buffer,
        width: u32,
        height: u32,
    ) -> (
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::BindGroup,
    ) {
        let w = width.clamp(64, 4096);
        let h = height.clamp(64, 4096);
        let extent = wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        };

        let msaa_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr_msaa_color"),
            size: extent,
            mip_level_count: 1,
            sample_count: MSAA_SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let hdr_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hdr_resolved_color"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("msaa_depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: MSAA_SAMPLES,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        let msaa_view = msaa_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let hdr_view = hdr_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tonemap_bg"),
            layout: tonemap_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(tonemap_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: tonemap_uniform_buf.as_entire_binding(),
                },
            ],
        });

        (msaa_view, hdr_view, depth_view, bg)
    }

    pub fn ensure_size(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let w = width.clamp(64, 4096);
        let h = height.clamp(64, 4096);
        if self.target_size != (w, h) {
            let (msaa_view, hdr_view, depth_view, bg) = Self::create_offscreen_targets(
                device,
                &self.tonemap_bgl,
                &self.tonemap_sampler,
                &self.tonemap_uniform_buf,
                w,
                h,
            );
            self.msaa_view = msaa_view;
            self.hdr_view = hdr_view;
            self.depth_view = depth_view;
            self.tonemap_bind_group = bg;
            self.target_size = (w, h);
        }
    }
}

pub struct WatershedPaintCallback {
    pub uniforms: SceneFrameUniforms,
    pub meshes: Arc<SceneMeshBuffers>,
    pub pixel_size: (u32, u32),
}

impl egui_wgpu::CallbackTrait for WatershedPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<WatershedGpuResources>() else {
            return Vec::new();
        };

        gpu.ensure_size(device, self.pixel_size.0, self.pixel_size.1);
        queue.write_buffer(&gpu.scene_uniform_buf, 0, &self.uniforms.to_le_bytes());

        let total_bytes =
            (self.meshes.opaque_bytes.len() + self.meshes.alpha_bytes.len()) as u64;
        if total_bytes == 0 {
            return Vec::new();
        }
        if total_bytes > gpu.vertex_capacity_bytes {
            let next_cap = (total_bytes * 2).max(4 * 1024 * 1024);
            gpu.vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("watershed_dynamic_vbuf_resized"),
                size: next_cap,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            gpu.vertex_capacity_bytes = next_cap;
        }

        let mut combined =
            Vec::with_capacity((total_bytes as usize).max(64));
        combined.extend_from_slice(&self.meshes.opaque_bytes);
        combined.extend_from_slice(&self.meshes.alpha_bytes);
        queue.write_buffer(&gpu.vertex_buf, 0, &combined);

        let opaque_count = self.meshes.opaque_vertex_count;
        let alpha_count = self.meshes.alpha_vertex_count;

        // Pass 1: 2048x2048 Directional Sun Shadow Map
        if opaque_count > 0 {
            let mut spass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("watershed_shadow_pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gpu.shadow_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            spass.set_pipeline(&gpu.shadow_pipeline);
            spass.set_bind_group(0, &gpu.shadow_bind_group, &[]);
            spass.set_vertex_buffer(0, gpu.vertex_buf.slice(..));
            spass.draw(0..opaque_count, 0..1);
        }

        // Pass 2: 4x MSAA HDR PBR + Creek Fresnel + Volumetric Sunbeams
        {
            let mut hpass = egui_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("watershed_hdr_msaa_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.msaa_view,
                    resolve_target: Some(&gpu.hdr_view),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.08,
                            g: 0.14,
                            b: 0.18,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &gpu.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            hpass.set_bind_group(0, &gpu.scene_bind_group, &[]);
            hpass.set_vertex_buffer(0, gpu.vertex_buf.slice(..));
            if opaque_count > 0 {
                hpass.set_pipeline(&gpu.opaque_pipeline);
                hpass.draw(0..opaque_count, 0..1);
            }
            if alpha_count > 0 {
                hpass.set_pipeline(&gpu.alpha_pipeline);
                hpass.draw(opaque_count..(opaque_count + alpha_count), 0..1);
            }
        }

        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<WatershedGpuResources>() else {
            return;
        };
        render_pass.set_pipeline(&gpu.tonemap_pipeline);
        render_pass.set_bind_group(0, &gpu.tonemap_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

pub fn build_sun_light_matrix(sun_dir: Vec3) -> Mat4 {
    let light_pos = sun_dir.normalize_or_zero() * 58.0;
    let view = Mat4::look_at_rh(light_pos, Vec3::ZERO, Vec3::Y);
    let proj = Mat4::orthographic_rh(-32.0, 32.0, -32.0, 32.0, 8.0, 125.0);
    proj * view
}
