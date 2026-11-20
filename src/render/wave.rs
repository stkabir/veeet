use std::mem::size_of;

mod fx;

use bytemuck::{Pod, Zeroable};
use eframe::egui;
use eframe::egui_wgpu::{self, wgpu};

use crate::audio::AnalysisFrame;
use crate::config::{VisualCfg, VisualPreset};

use super::alchemy::{AlchemyMotion, FeedbackParams};
use super::ambience::{AmbienceMotion, AmbienceParams};
use super::palette;

const MAX_VERTICES: usize = 120_000;
const BLOOM_MIPS: usize = 5;
const KERNEL_RADII: [u32; BLOOM_MIPS] = [3, 5, 7, 9, 11];
const KERNEL_STRIDE: usize = 11;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

const VERTEX_ATTRS: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Globals {
    resolution: [f32; 2],
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HighPassParams {
    threshold: f32,
    smooth_width: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurParams {
    inv_size: [f32; 2],
    direction: [f32; 2],
    kernel_radius: u32,
    mip: u32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BloomParams {
    strength: f32,
    radius: f32,
    _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FinalParams {
    exposure: f32,
    black_bg: f32,
    grid_enabled: f32,
    grid_rows: f32,
    grid_overlap: f32,
    grid_opacity: f32,
    hue_shift: f32,
    saturation: f32,
    contrast: f32,
    tint_amount: f32,
    vignette: f32,
    /// Bit 0: espejo horizontal; bit 1: espejo vertical.
    mirror: f32,
    /// Encuadre: tamaño de la escena y desplazamiento.
    scale: f32,
    _pad: f32,
    /// Alineado a 8 bytes, como `vec2f` en WGSL.
    offset: [f32; 2],
    /// RGB lineal + relleno.
    tint: [f32; 4],
    background: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<FinalParams>() == 96);

/// Ajustes de color del post-proceso final, por preset.
#[derive(Clone, Copy)]
struct ColorGrade {
    hue_shift: f32,
    saturation: f32,
    contrast: f32,
    tint_amount: f32,
    vignette: f32,
    mirror: f32,
    scale: f32,
    offset: [f32; 2],
    tint: [f32; 4],
    background: [f32; 4],
}

impl Default for ColorGrade {
    fn default() -> Self {
        Self {
            hue_shift: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            tint_amount: 0.0,
            vignette: 0.0,
            mirror: 0.0,
            scale: 1.0,
            offset: [0.0; 2],
            tint: [1.0; 4],
            background: [0.0, 0.0, 0.0, 1.0],
        }
    }
}

fn srgb_to_linear(c: [u8; 3]) -> [f32; 4] {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    [f(c[0]), f(c[1]), f(c[2]), 1.0]
}

struct Offscreen {
    width: u32,
    height: u32,
    samples: u32,
    msaa_view: Option<wgpu::TextureView>,
    /// Se conserva la textura para copiar el feedback de Alquimia encima.
    scene_texture: wgpu::Texture,
    scene_view: wgpu::TextureView,
    /// Ping-pong del feedback de Alquimia.
    feedback_textures: [wgpu::Texture; 2],
    feedback_views: [wgpu::TextureView; 2],
    /// `feedback_binds[i]` lee `feedback_views[i]` (frame anterior) y la escena.
    feedback_binds: [wgpu::BindGroup; 2],
    bright_view: wgpu::TextureView,
    h_views: Vec<wgpu::TextureView>,
    v_views: Vec<wgpu::TextureView>,
    blur_sizes: Vec<(u32, u32)>,
    highpass_bind: wgpu::BindGroup,
    blur_binds: Vec<wgpu::BindGroup>,
    bloom_bind: wgpu::BindGroup,
    copy_bind: wgpu::BindGroup,
    final_bind: wgpu::BindGroup,
}

pub struct WaveRenderer {
    vertex_buffer: wgpu::Buffer,
    globals_buffer: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    highpass_buffer: wgpu::Buffer,
    blur_buffers: Vec<wgpu::Buffer>,
    bloom_buffer: wgpu::Buffer,
    final_buffer: wgpu::Buffer,
    coeffs_buffer: wgpu::Buffer,
    core_pipeline: wgpu::RenderPipeline,
    highpass_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    bloom_pipeline: wgpu::RenderPipeline,
    add_pipeline: wgpu::RenderPipeline,
    final_pipeline: wgpu::RenderPipeline,
    alchemy_pipeline: wgpu::RenderPipeline,
    alchemy_layout: wgpu::BindGroupLayout,
    alchemy_buffer: wgpu::Buffer,
    highpass_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    bloom_layout: wgpu::BindGroupLayout,
    copy_layout: wgpu::BindGroupLayout,
    final_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    samples: u32,
    offscreen: Option<Offscreen>,
    scene: Vec<Vertex>,
    vertex_count: u32,
    glow: f32,
    exposure: f32,
    black_bg: f32,
    grid_enabled: f32,
    grid_rows: f32,
    grid_overlap: f32,
    grid_opacity: f32,
    bloom_threshold: f32,
    bloom_radius: f32,
    grade: ColorGrade,
    /// Buffer reutilizado para agrupar bandas (sin allocs por frame).
    scratch: Vec<f32>,
    /// Ambiente: audio suavizado, flujo de las nubes y su uniform.
    ambience_pipeline: wgpu::RenderPipeline,
    /// Misma shader con mezcla aditiva: la nebulosa del Huracán se suma a la
    /// escena después del feedback, así no se acumula en las estelas.
    ambience_add_pipeline: wgpu::RenderPipeline,
    ambience_buffer: wgpu::Buffer,
    ambience_bind: wgpu::BindGroup,
    ambience_motion: AmbienceMotion,
    ambience_params: AmbienceParams,
    ambience_mode: AmbienceMode,
    ambience_last_time: f32,
    /// Ciclo de modos de Alquimia.
    alchemy_motion: AlchemyMotion,
    alchemy_last_time: f32,
    /// Estado de los presets con feedback (Anillo, Ocean Mist, Scope,
    /// Battery, Huracán).
    fx: fx::FxState,
    /// Parámetros del feedback de GPU del preset activo, si lo usa.
    feedback_params: FeedbackParams,
    feedback_active: bool,
    /// Preset del frame anterior: al cambiar se limpia el feedback.
    last_preset: Option<VisualPreset>,
    /// Índice del feedback que guarda el frame anterior.
    feedback_index: usize,
    /// Limpiar el feedback antes de usarlo (al entrar al preset o redimensionar).
    feedback_clear: bool,
    dirty: bool,
}

/// Cómo entra el shader de pantalla completa (`ambience.wgsl`) en el frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AmbienceMode {
    Off,
    /// Reemplaza la escena (Ambiente).
    Replace,
    /// Se suma sobre la escena ya con feedback (nebulosa del Huracán).
    Additive,
}

impl WaveRenderer {
    pub fn new(
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        target_format: wgpu::TextureFormat,
    ) -> Self {
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet line shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("wave.wgsl").into()),
        });
        let highpass_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet highpass shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("highpass.wgsl").into()),
        });
        let blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("blur.wgsl").into()),
        });
        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet bloom composite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bloom.wgsl").into()),
        });
        let copy_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet copy shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("copy.wgsl").into()),
        });
        let final_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet final shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("final.wgsl").into()),
        });

        let alchemy_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet alchemy shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("alchemy.wgsl").into()),
        });

        let ambience_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("veeet ambience shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("ambience.wgsl").into()),
        });

        let samples = if adapter
            .get_texture_format_features(wgpu::TextureFormat::Rgba16Float)
            .flags
            .sample_count_supported(4)
        {
            4
        } else {
            1
        };

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet globals layout"),
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

        let highpass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet highpass layout"),
            entries: &[texture_entry(0), sampler_entry(1), uniform_entry(2)],
        });

        let blur_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet blur layout"),
            entries: &[
                texture_entry(0),
                sampler_entry(1),
                uniform_entry(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet bloom layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                texture_entry(2),
                texture_entry(3),
                texture_entry(4),
                sampler_entry(5),
                uniform_entry(6),
            ],
        });

        let copy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet copy layout"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });

        let final_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet final layout"),
            entries: &[texture_entry(0), sampler_entry(1), uniform_entry(2)],
        });

        // alchemy.wgsl: prev_tex (0), scene_tex (1), samp (2), fb (3).
        let alchemy_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet alchemy layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                sampler_entry(2),
                uniform_entry(3),
            ],
        });
        let alchemy_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("veeet alchemy layout"),
                bind_group_layouts: &[Some(&alchemy_layout)],
                immediate_size: 0,
            });

        // ambience.wgsl: amb (0).
        let ambience_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("veeet ambience layout"),
            entries: &[uniform_entry(0)],
        });
        let ambience_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("veeet ambience layout"),
                bind_group_layouts: &[Some(&ambience_layout)],
                immediate_size: 0,
            });

        let line_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("veeet line layout"),
            bind_group_layouts: &[Some(&globals_layout)],
            immediate_size: 0,
        });
        let highpass_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("veeet highpass layout"),
                bind_group_layouts: &[Some(&highpass_layout)],
                immediate_size: 0,
            });
        let blur_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("veeet blur layout"),
            bind_group_layouts: &[Some(&blur_layout)],
            immediate_size: 0,
        });
        let bloom_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("veeet bloom layout"),
                bind_group_layouts: &[Some(&bloom_layout)],
                immediate_size: 0,
            });
        let copy_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("veeet copy layout"),
            bind_group_layouts: &[Some(&copy_layout)],
            immediate_size: 0,
        });
        let final_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("veeet final layout"),
                bind_group_layouts: &[Some(&final_layout)],
                immediate_size: 0,
            });

        let core_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("veeet core pipeline"),
            layout: Some(&line_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_line"),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRS,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_line"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        let highpass_pipeline = fullscreen_pipeline(
            device,
            "veeet highpass pipeline",
            &highpass_pipeline_layout,
            &highpass_shader,
            "fs_highpass",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState::REPLACE),
        );
        let blur_pipeline = fullscreen_pipeline(
            device,
            "veeet blur pipeline",
            &blur_pipeline_layout,
            &blur_shader,
            "fs_blur",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState::REPLACE),
        );
        let bloom_pipeline = fullscreen_pipeline(
            device,
            "veeet bloom pipeline",
            &bloom_pipeline_layout,
            &bloom_shader,
            "fs_bloom_composite",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState::REPLACE),
        );
        let add_pipeline = fullscreen_pipeline(
            device,
            "veeet bloom add pipeline",
            &copy_pipeline_layout,
            &copy_shader,
            "fs_copy",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            }),
        );
        let final_pipeline = fullscreen_pipeline(
            device,
            "veeet final pipeline",
            &final_pipeline_layout,
            &final_shader,
            "fs_final",
            target_format,
            Some(wgpu::BlendState {
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
            }),
        );

        let alchemy_pipeline = fullscreen_pipeline(
            device,
            "veeet alchemy pipeline",
            &alchemy_pipeline_layout,
            &alchemy_shader,
            "fs_feedback",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState::REPLACE),
        );

        let ambience_pipeline = fullscreen_pipeline(
            device,
            "veeet ambience pipeline",
            &ambience_pipeline_layout,
            &ambience_shader,
            "fs_ambience",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState::REPLACE),
        );
        let additive = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let ambience_add_pipeline = fullscreen_pipeline(
            device,
            "veeet ambience add pipeline",
            &ambience_pipeline_layout,
            &ambience_shader,
            "fs_ambience",
            wgpu::TextureFormat::Rgba16Float,
            Some(wgpu::BlendState {
                color: additive,
                alpha: additive,
            }),
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("veeet sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let make_uniform = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };

        let globals_buffer = make_uniform("veeet globals", size_of::<Globals>() as u64);
        let highpass_buffer = make_uniform("veeet highpass", size_of::<HighPassParams>() as u64);
        let mut blur_buffers = Vec::with_capacity(BLOOM_MIPS * 2);
        for i in 0..BLOOM_MIPS * 2 {
            blur_buffers.push(make_uniform(
                &format!("veeet blur {i}"),
                size_of::<BlurParams>() as u64,
            ));
        }
        let bloom_buffer = make_uniform("veeet bloom", size_of::<BloomParams>() as u64);
        let final_buffer = make_uniform("veeet final", size_of::<FinalParams>() as u64);
        let alchemy_buffer = make_uniform("veeet alchemy", size_of::<FeedbackParams>() as u64);
        let coeffs_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("veeet bloom coefficients"),
            size: (BLOOM_MIPS * KERNEL_STRIDE * size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("veeet globals bind"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let ambience_buffer = make_uniform("veeet ambience", size_of::<AmbienceParams>() as u64);
        let ambience_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("veeet ambience bind"),
            layout: &ambience_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ambience_buffer.as_entire_binding(),
            }],
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("veeet vertices"),
            size: (MAX_VERTICES * size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            vertex_buffer,
            globals_buffer,
            globals_bind,
            highpass_buffer,
            blur_buffers,
            bloom_buffer,
            final_buffer,
            coeffs_buffer,
            core_pipeline,
            highpass_pipeline,
            blur_pipeline,
            bloom_pipeline,
            add_pipeline,
            final_pipeline,
            alchemy_pipeline,
            alchemy_layout,
            alchemy_buffer,
            highpass_layout,
            blur_layout,
            bloom_layout,
            copy_layout,
            final_layout,
            sampler,
            samples,
            offscreen: None,
            scene: Vec::with_capacity(MAX_VERTICES),
            vertex_count: 0,
            glow: 2.0,
            exposure: 1.0,
            black_bg: 1.0,
            grid_enabled: 1.0,
            grid_rows: 3.0,
            grid_overlap: 0.34,
            grid_opacity: 0.10,
            bloom_threshold: 0.2,
            bloom_radius: 0.3,
            grade: ColorGrade::default(),
            scratch: Vec::with_capacity(64),
            ambience_pipeline,
            ambience_add_pipeline,
            ambience_buffer,
            ambience_bind,
            ambience_motion: AmbienceMotion::new(),
            ambience_params: AmbienceParams::zeroed(),
            ambience_mode: AmbienceMode::Off,
            ambience_last_time: 0.0,
            alchemy_motion: AlchemyMotion::new(),
            fx: fx::FxState::new(),
            feedback_params: FeedbackParams::zeroed(),
            feedback_active: false,
            last_preset: None,
            alchemy_last_time: 0.0,
            feedback_index: 0,
            feedback_clear: true,
            dirty: false,
        }
    }

    /// `fade` (0..1) apaga la escena: el fundido del modo aleatorio.
    #[allow(clippy::too_many_arguments)]
    pub fn update_scene(
        &mut self,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        width: f32,
        height: f32,
        time: f32,
        pixels_per_point: f32,
        fade: f32,
    ) {
        self.scene.clear();
        self.vertex_count = 0;

        if width < 2.0 || height < 2.0 {
            return;
        }

        let ppp = pixels_per_point.max(0.5);
        match cfg.preset {
            VisualPreset::WavyForm3 => {
                self.build_wave(analysis, cfg, width, height, time, ppp);
            }
            VisualPreset::Alchemy => {
                self.build_alchemy(analysis, cfg, width, height, time);
            }
            VisualPreset::Ambience => {
                self.update_ambience(analysis, cfg, time, 0.0, 0.0);
            }
            VisualPreset::WaveRing
            | VisualPreset::Scope
            | VisualPreset::Hurricane
            | VisualPreset::Niagara
            | VisualPreset::Windmill => {
                let frame = fx::Frame {
                    analysis,
                    cfg,
                    width,
                    height,
                    time,
                    ppp,
                };
                self.feedback_params = self.fx.build(&mut self.scene, &frame);
                if cfg.preset == VisualPreset::Hurricane {
                    // Radio del ojo en unidades del shader (alto = 1).
                    let eye = cfg.hurricane_eye * width.min(height) / height;
                    self.update_ambience(analysis, cfg, time, 1.0, eye);
                }
            }
        }
        self.ambience_mode = match cfg.preset {
            VisualPreset::Ambience => AmbienceMode::Replace,
            VisualPreset::Hurricane => AmbienceMode::Additive,
            VisualPreset::WavyForm3
            | VisualPreset::WaveRing
            | VisualPreset::Alchemy
            | VisualPreset::Scope
            | VisualPreset::Niagara
            | VisualPreset::Windmill => AmbienceMode::Off,
        };
        self.feedback_active = matches!(
            cfg.preset,
            VisualPreset::WaveRing
                | VisualPreset::Alchemy
                | VisualPreset::Scope
                | VisualPreset::Hurricane
                | VisualPreset::Niagara
                | VisualPreset::Windmill
        );
        if self.last_preset != Some(cfg.preset) {
            self.feedback_clear = true;
            self.last_preset = Some(cfg.preset);
        }

        let usable = (self.scene.len().min(MAX_VERTICES) / 3) * 3;
        self.scene.truncate(usable);
        self.vertex_count = usable as u32;

        self.glow = match cfg.preset {
            VisualPreset::WavyForm3 => cfg.glow,
            VisualPreset::Alchemy => cfg.alchemy_glow,
            VisualPreset::Ambience => cfg.ambience_glow,
            VisualPreset::WaveRing | VisualPreset::Scope | VisualPreset::Hurricane => cfg.fx_glow,
            // Los originales de WMP no tienen bloom: apenas un toque.
            VisualPreset::Niagara | VisualPreset::Windmill => cfg.fx_glow * 0.25,
        }
        .max(0.0);
        self.exposure = cfg.exposure.max(0.0) * fade.clamp(0.0, 1.0);
        self.black_bg = 1.0;
        self.grid_enabled = if cfg.grid_tile { 1.0 } else { 0.0 };
        let authored_rows = cfg.grid_tile_rows.max(1) as f32;
        let logical_height = height / ppp;
        self.grid_rows = (authored_rows * logical_height / 900.0)
            .round()
            .clamp(1.0, authored_rows);
        self.grid_overlap = cfg.grid_tile_overlap.clamp(0.0, 1.0);
        self.grid_opacity = cfg.grid_tile_opacity.clamp(0.0, 1.0);
        self.bloom_threshold = cfg.bloom_threshold.clamp(0.0, 1.0);
        self.bloom_radius = cfg.bloom_radius.clamp(0.0, 1.0);
        self.grade = ColorGrade {
            hue_shift: cfg.hue_shift,
            saturation: cfg.saturation.max(0.0),
            contrast: cfg.contrast.max(0.0),
            tint_amount: cfg.tint_amount.clamp(0.0, 1.0),
            vignette: cfg.vignette.clamp(0.0, 1.0),
            mirror: (cfg.mirror_x as u32 | (cfg.mirror_y as u32) << 1) as f32,
            scale: cfg.scene_scale.max(0.05),
            offset: [cfg.offset_x, cfg.offset_y],
            tint: srgb_to_linear(cfg.tint),
            background: srgb_to_linear(cfg.background),
        };
        self.dirty = true;
    }

    fn build_wave(
        &mut self,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        width: f32,
        height: f32,
        time: f32,
        ppp: f32,
    ) {
        let n = analysis.waveform.len().min(analysis.spectrum.len());
        if n < 2 {
            return;
        }

        let aspect = width / height;
        let fov = 50.0_f32.to_radians();
        let half_tan = (fov * 0.5).tan();
        let camera_z = cfg.camera_z.max(0.1);

        let visible_half_height = camera_z * half_tan;
        let visible_half_width = visible_half_height * aspect;
        let group_width = cfg.wave_width * if aspect > 1.0 { 5.0 } else { 5.0 / aspect };
        let x_projection = (width * 0.5) / visible_half_width;
        let y_projection = (height * 0.5) / visible_half_height;

        let center_x = width * 0.5;
        let center_y = height * 0.5;
        let half_width = (cfg.line_width * ppp * 0.5).max(0.5);
        let sensitivity = cfg.sensitivity.max(0.0);
        let variation = cfg.line_variation.max(0.0);
        let variation_speed = cfg.variation_speed;
        let variation_time = time * variation_speed;
        let line_count = cfg.line_count.clamp(1, 500) as usize;
        let custom = palette::custom(cfg);
        let drift = time * 0.02 * cfg.color_cycle;

        for layer in 0..line_count {
            let phase = layer as f32 / line_count as f32 * std::f32::consts::TAU;
            let mut prev: Option<(f32, f32, [f32; 4])> = None;

            for i in 0..n {
                let value = analysis.waveform[i];
                let freq = analysis.spectrum[i].clamp(0.0, 1.0);
                let u = i as f32 / n as f32;

                let edge = 1.0 + (1.0 - 2.0 * (u - 0.5).abs()).powi(2);
                let y_value = value * sensitivity * edge;

                let mut local_x = (u - 0.5) * group_width;
                let mut local_y = y_value * 2.0;

                if variation > 0.0 {
                    let vx = (2.0 * variation_time + phase + 10.0 * u).sin() * variation * 0.02;
                    let vy = (1.5 * variation_time + phase + 8.0 * u).sin() * variation * 0.15
                        + (variation_time + phase).sin() * variation * 0.2;
                    local_x += vx * group_width;
                    local_y += vy * 2.0;
                }

                let x = center_x + local_x * x_projection;
                let y = center_y + local_y * y_projection;

                let v = value.abs();
                let g = 1.4 * freq;
                let hue = 1.0 - ((v + g) * 1.2).min(1.0);
                let lightness = (0.4 + 0.4 * v.max(g)).min(0.8);
                let rgb = match &custom {
                    Some(colors) => {
                        scale_rgb3(palette::cycle3(colors, hue * 0.67 + drift), lightness * 1.6)
                    }
                    None => {
                        let saturation = (0.85 + 0.15 * g).min(1.0);
                        hsl_to_rgb(hue, saturation, lightness)
                    }
                };
                let color = [rgb[0] * 1.3, rgb[1] * 1.3, rgb[2] * 1.3, 1.0];

                if let Some((px, py, pc)) = prev {
                    push_segment(&mut self.scene, (px, py), (x, y), pc, color, half_width);
                }
                prev = Some((x, y, color));
            }
        }
    }

    /// Ambiente no dibuja geometría: avanza el flujo de las nubes y prepara
    /// el uniform del shader de pantalla completa.
    fn update_ambience(
        &mut self,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        time: f32,
        mode: f32,
        eye_radius: f32,
    ) {
        let mut tri = std::mem::take(&mut self.scratch);
        resample_bands(&analysis.bands, 3, &mut tri);
        let bands = [
            tri.first().copied().unwrap_or(0.0),
            tri.get(1).copied().unwrap_or(0.0),
            tri.get(2).copied().unwrap_or(0.0),
        ];
        self.scratch = tri;
        let dt = (time - self.ambience_last_time).clamp(0.001, 0.1);
        self.ambience_last_time = time;
        self.ambience_params = self
            .ambience_motion
            .update(dt, analysis, cfg, bands, mode, eye_radius);
    }

    /// Alquimia: curvas tipo rosa/Lissajous moduladas por graves, medios y
    /// agudos, una estrella en cada beat y partículas en órbita. El feedback
    /// de GPU convierte estos trazos en estelas que fluyen.
    fn build_alchemy(
        &mut self,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        width: f32,
        height: f32,
        time: f32,
    ) {
        let mut tri = std::mem::take(&mut self.scratch);
        resample_bands(&analysis.bands, 3, &mut tri);
        let bands = [
            tri.first().copied().unwrap_or(0.0),
            tri.get(1).copied().unwrap_or(0.0),
            tri.get(2).copied().unwrap_or(0.0),
        ];
        self.scratch = tri;
        let [bass, mid, treble] = bands;

        let dt = (time - self.alchemy_last_time).clamp(0.001, 0.1);
        self.alchemy_last_time = time;
        self.feedback_params = self.alchemy_motion.update(dt, time, analysis, cfg, bands);
        let hue = self.alchemy_motion.hue();

        let tau = std::f32::consts::TAU;
        let speed = cfg.alchemy_speed;
        let t = time * speed;
        let size = cfg.alchemy_size;
        let custom = palette::custom(cfg);
        let paint = |t: f32| match &custom {
            Some(colors) => palette::cycle3(colors, t),
            None => palette::sample(palette::ALCHEMY, t.rem_euclid(1.0)),
        };
        let (cx, cy) = (width * 0.5, height * 0.5);
        let min_dim = width.min(height);
        let level = analysis.level.clamp(0.0, 1.0);
        let beat = analysis.beat_env.clamp(0.0, 1.0);
        let curves = cfg.alchemy_complexity.max(1) as usize;
        const SEGMENTS: usize = 180;

        for c in 0..curves {
            let k = c as f32 / curves as f32;
            let petals = (2 + c % 4) as f32;
            let dir = if c % 2 == 0 { 1.0 } else { -1.0 };
            let base_r = min_dim * size * (0.10 + 0.24 * k) * (1.0 + 0.5 * bass + 0.2 * beat);
            let amp = min_dim * size * 0.05 * (0.3 + 1.4 * mid);
            let phase = t * (0.35 + 0.2 * c as f32) * dir;
            let squash = 0.85 + 0.15 * (t * 0.4 + k * tau).sin();
            let rgb = paint(hue + k * 0.35);
            let color = scale_rgb(rgb, 0.45 + 0.5 * level + 0.35 * beat);
            let half_w = 0.8 + 0.7 * level;

            let point = |i: usize| {
                let th = i as f32 / SEGMENTS as f32 * tau;
                let r = base_r
                    + amp * (petals * th + phase).sin()
                    + amp * treble * 0.35 * (13.0 * th - t * 3.0).sin();
                let a = th + phase * 0.3;
                (cx + r * a.cos(), cy + r * a.sin() * squash)
            };
            for i in 0..SEGMENTS {
                push_segment(
                    &mut self.scene,
                    point(i),
                    point(i + 1),
                    color,
                    color,
                    half_w,
                );
            }
        }

        // Estrella en el beat: rayos que nacen del centro.
        if cfg.alchemy_star && beat > 0.05 {
            let rays = 12;
            let rgb = paint(hue + 0.5);
            let color = scale_rgb(rgb, 0.9 * beat);
            for i in 0..rays {
                let a = i as f32 / rays as f32 * tau + t * 0.5;
                let d = (a.cos(), a.sin());
                let r0 = min_dim * 0.04;
                let r1 = r0 + min_dim * size * 0.42 * beat;
                push_segment(
                    &mut self.scene,
                    (cx + d.0 * r0, cy + d.1 * r0),
                    (cx + d.0 * r1, cy + d.1 * r1),
                    color,
                    scale_rgb(rgb, 0.1 * beat),
                    0.8,
                );
            }
        }

        // Partículas en órbita.
        let particles = cfg.alchemy_particles as usize;
        for i in 0..particles {
            let f = i as f32;
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let a = t * (0.5 + 0.12 * f) * dir + f * 2.399_963;
            let r = min_dim * size * (0.16 + 0.26 * (f * 0.37).fract()) * (1.0 + 0.3 * bass);
            let rgb = paint(hue + f * 0.13);
            push_disc(
                &mut self.scene,
                (cx + r * a.cos(), cy + r * a.sin()),
                1.5 + 3.0 * level,
                scale_rgb(rgb, 0.6 + 0.6 * level),
                8,
            );
        }
    }

    fn ensure_offscreen(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if let Some(off) = &self.offscreen
            && off.width == width
            && off.height == height
            && off.samples == self.samples
        {
            return;
        }

        let samples = self.samples;
        let (scene_texture, scene_view) = create_texture(
            device,
            "veeet scene",
            width,
            height,
            wgpu::TextureFormat::Rgba16Float,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
        );
        let make_feedback = |label: &str| {
            create_texture(
                device,
                label,
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
            )
        };
        let (feedback_a, feedback_a_view) = make_feedback("veeet alchemy feedback a");
        let (feedback_b, feedback_b_view) = make_feedback("veeet alchemy feedback b");
        let feedback_binds = [&feedback_a_view, &feedback_b_view].map(|prev| {
            create_feedback_bind(
                device,
                &self.alchemy_layout,
                prev,
                &scene_view,
                &self.sampler,
                &self.alchemy_buffer,
            )
        });
        self.feedback_clear = true;
        self.feedback_index = 0;

        let msaa_view = if samples > 1 {
            let (msaa_texture, view) = create_texture(
                device,
                "veeet scene msaa",
                width,
                height,
                wgpu::TextureFormat::Rgba16Float,
                samples,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let _ = msaa_texture;
            Some(view)
        } else {
            None
        };

        let mut w = ((width as f32) * 0.5).round().max(1.0) as u32;
        let mut h = ((height as f32) * 0.5).round().max(1.0) as u32;
        let (bright_texture, bright_view) = create_texture(
            device,
            "veeet bloom bright",
            w,
            h,
            wgpu::TextureFormat::Rgba16Float,
            1,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let _ = bright_texture;

        let mut h_views = Vec::with_capacity(BLOOM_MIPS);
        let mut v_views = Vec::with_capacity(BLOOM_MIPS);
        let mut blur_sizes = Vec::with_capacity(BLOOM_MIPS);
        for _ in 0..BLOOM_MIPS {
            let (h_texture, h_view) = create_texture(
                device,
                "veeet bloom h",
                w,
                h,
                wgpu::TextureFormat::Rgba16Float,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let (v_texture, v_view) = create_texture(
                device,
                "veeet bloom v",
                w,
                h,
                wgpu::TextureFormat::Rgba16Float,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let _ = (h_texture, v_texture);
            h_views.push(h_view);
            v_views.push(v_view);
            blur_sizes.push((w, h));
            w = ((w as f32) * 0.5).round().max(1.0) as u32;
            h = ((h as f32) * 0.5).round().max(1.0) as u32;
        }

        let highpass_bind = create_highpass_bind(
            device,
            &self.highpass_layout,
            &scene_view,
            &self.sampler,
            &self.highpass_buffer,
        );

        let mut blur_binds = Vec::with_capacity(BLOOM_MIPS * 2);
        for i in 0..BLOOM_MIPS {
            let source = if i == 0 {
                &bright_view
            } else {
                &v_views[i - 1]
            };
            blur_binds.push(create_blur_bind(
                device,
                &self.blur_layout,
                source,
                &self.sampler,
                &self.blur_buffers[2 * i],
                &self.coeffs_buffer,
            ));
            blur_binds.push(create_blur_bind(
                device,
                &self.blur_layout,
                &h_views[i],
                &self.sampler,
                &self.blur_buffers[2 * i + 1],
                &self.coeffs_buffer,
            ));
        }

        let bloom_bind = create_bloom_bind(
            device,
            &self.bloom_layout,
            &v_views,
            &self.sampler,
            &self.bloom_buffer,
        );
        let copy_bind = create_copy_bind(device, &self.copy_layout, &h_views[0], &self.sampler);
        let final_bind = create_final_bind(
            device,
            &self.final_layout,
            &scene_view,
            &self.sampler,
            &self.final_buffer,
        );

        self.offscreen = Some(Offscreen {
            width,
            height,
            samples,
            msaa_view,
            scene_texture,
            scene_view,
            feedback_textures: [feedback_a, feedback_b],
            feedback_views: [feedback_a_view, feedback_b_view],
            feedback_binds,
            bright_view,
            h_views,
            v_views,
            blur_sizes,
            highpass_bind,
            blur_binds,
            bloom_bind,
            copy_bind,
            final_bind,
        });
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let [width, height] = screen.size_in_pixels;
        if width == 0 || height == 0 {
            return;
        }
        self.ensure_offscreen(device, width, height);
        let Some(off) = &self.offscreen else {
            return;
        };

        if self.dirty && self.vertex_count > 0 {
            let data = bytemuck::cast_slice(&self.scene[..self.vertex_count as usize]);
            queue.write_buffer(&self.vertex_buffer, 0, data);
            self.dirty = false;
        }

        let globals = Globals {
            resolution: [width as f32, height as f32],
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        let highpass = HighPassParams {
            threshold: self.bloom_threshold,
            smooth_width: 0.01,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.highpass_buffer, 0, bytemuck::bytes_of(&highpass));

        for (i, (&radius, &(w, h))) in KERNEL_RADII.iter().zip(off.blur_sizes.iter()).enumerate() {
            let inv_size = [1.0 / w as f32, 1.0 / h as f32];
            let horizontal = BlurParams {
                inv_size,
                direction: [1.0, 0.0],
                kernel_radius: radius,
                mip: i as u32,
                _pad: [0.0; 2],
            };
            let vertical = BlurParams {
                inv_size,
                direction: [0.0, 1.0],
                kernel_radius: radius,
                mip: i as u32,
                _pad: [0.0; 2],
            };
            queue.write_buffer(
                &self.blur_buffers[2 * i],
                0,
                bytemuck::bytes_of(&horizontal),
            );
            queue.write_buffer(
                &self.blur_buffers[2 * i + 1],
                0,
                bytemuck::bytes_of(&vertical),
            );
        }

        let coeffs = bloom_coefficients();
        queue.write_buffer(&self.coeffs_buffer, 0, bytemuck::cast_slice(&coeffs[..]));

        let bloom = BloomParams {
            strength: self.glow,
            radius: self.bloom_radius,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&self.bloom_buffer, 0, bytemuck::bytes_of(&bloom));

        let final_params = FinalParams {
            exposure: self.exposure,
            black_bg: self.black_bg,
            grid_enabled: self.grid_enabled,
            grid_rows: self.grid_rows,
            grid_overlap: self.grid_overlap,
            grid_opacity: self.grid_opacity,
            hue_shift: self.grade.hue_shift,
            saturation: self.grade.saturation,
            contrast: self.grade.contrast,
            tint_amount: self.grade.tint_amount,
            vignette: self.grade.vignette,
            mirror: self.grade.mirror,
            scale: self.grade.scale,
            _pad: 0.0,
            offset: self.grade.offset,
            tint: self.grade.tint,
            background: self.grade.background,
        };
        queue.write_buffer(&self.final_buffer, 0, bytemuck::bytes_of(&final_params));

        let clear = wgpu::Color {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        };

        {
            let target_view = if off.samples > 1 {
                off.msaa_view.as_ref().expect("falta la vista MSAA")
            } else {
                &off.scene_view
            };
            let resolve_target = if off.samples > 1 {
                Some(&off.scene_view)
            } else {
                None
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("veeet core pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if self.vertex_count > 0 {
                pass.set_pipeline(&self.core_pipeline);
                pass.set_bind_group(0, &self.globals_bind, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..self.vertex_count, 0..1);
            }
        }

        if self.ambience_mode != AmbienceMode::Off {
            let params = AmbienceParams {
                resolution: [width as f32, height as f32],
                ..self.ambience_params
            };
            queue.write_buffer(&self.ambience_buffer, 0, bytemuck::bytes_of(&params));
        }
        if self.ambience_mode == AmbienceMode::Replace {
            fullscreen_pass(
                encoder,
                &self.ambience_pipeline,
                &off.scene_view,
                &self.ambience_bind,
                wgpu::LoadOp::Clear(clear),
                "veeet ambience",
            );
        }

        if self.feedback_active {
            let params = FeedbackParams {
                resolution: [width as f32, height as f32],
                ..self.feedback_params
            };
            queue.write_buffer(&self.alchemy_buffer, 0, bytemuck::bytes_of(&params));
            if self.feedback_clear {
                for view in &off.feedback_views {
                    clear_pass(encoder, view, "veeet alchemy clear");
                }
                self.feedback_clear = false;
            }
            let prev = self.feedback_index;
            let next = 1 - prev;
            fullscreen_pass(
                encoder,
                &self.alchemy_pipeline,
                &off.feedback_views[next],
                &off.feedback_binds[prev],
                wgpu::LoadOp::Clear(clear),
                "veeet alchemy feedback",
            );
            // El resultado reemplaza la escena: bloom y salida no cambian.
            encoder.copy_texture_to_texture(
                off.feedback_textures[next].as_image_copy(),
                off.scene_texture.as_image_copy(),
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.feedback_index = next;
        }

        if self.ambience_mode == AmbienceMode::Additive {
            fullscreen_pass(
                encoder,
                &self.ambience_add_pipeline,
                &off.scene_view,
                &self.ambience_bind,
                wgpu::LoadOp::Load,
                "veeet ambience add",
            );
        }

        fullscreen_pass(
            encoder,
            &self.highpass_pipeline,
            &off.bright_view,
            &off.highpass_bind,
            wgpu::LoadOp::Clear(clear),
            "veeet highpass",
        );

        for i in 0..BLOOM_MIPS {
            fullscreen_pass(
                encoder,
                &self.blur_pipeline,
                &off.h_views[i],
                &off.blur_binds[2 * i],
                wgpu::LoadOp::Clear(clear),
                "veeet bloom blur h",
            );
            fullscreen_pass(
                encoder,
                &self.blur_pipeline,
                &off.v_views[i],
                &off.blur_binds[2 * i + 1],
                wgpu::LoadOp::Clear(clear),
                "veeet bloom blur v",
            );
        }

        fullscreen_pass(
            encoder,
            &self.bloom_pipeline,
            &off.h_views[0],
            &off.bloom_bind,
            wgpu::LoadOp::Clear(clear),
            "veeet bloom composite",
        );

        fullscreen_pass(
            encoder,
            &self.add_pipeline,
            &off.scene_view,
            &off.copy_bind,
            wgpu::LoadOp::Load,
            "veeet bloom add",
        );
    }

    pub fn paint(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        let Some(off) = &self.offscreen else {
            return;
        };
        render_pass.set_pipeline(&self.final_pipeline);
        render_pass.set_bind_group(0, &off.final_bind, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

pub struct WaveCallback;

impl egui_wgpu::CallbackTrait for WaveCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = callback_resources.get_mut::<WaveRenderer>() {
            renderer.prepare(device, queue, screen_descriptor, egui_encoder);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = callback_resources.get::<WaveRenderer>() {
            renderer.paint(render_pass);
        }
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
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

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn fullscreen_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &str,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

fn create_texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    sample_count: u32,
    usage: wgpu::TextureUsages,
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

fn create_highpass_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet highpass bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

fn create_blur_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
    coeffs: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet blur bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: coeffs.as_entire_binding(),
            },
        ],
    })
}

fn create_bloom_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    views: &[wgpu::TextureView],
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    debug_assert_eq!(views.len(), BLOOM_MIPS);
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet bloom bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&views[0]),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&views[1]),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&views[2]),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&views[3]),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&views[4]),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

fn create_copy_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet copy bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_final_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet final bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

fn create_feedback_bind(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    prev: &wgpu::TextureView,
    scene: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
    params: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("veeet alchemy bind"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(prev),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(scene),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: params.as_entire_binding(),
            },
        ],
    })
}

/// Pase vacío que solo limpia `target` a negro.
fn clear_pass(encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView, label: &str) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}

fn fullscreen_pass(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::RenderPipeline,
    target: &wgpu::TextureView,
    bind_group: &wgpu::BindGroup,
    load: wgpu::LoadOp<wgpu::Color>,
    label: &str,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

fn bloom_coefficients() -> [f32; BLOOM_MIPS * KERNEL_STRIDE] {
    let mut out = [0.0f32; BLOOM_MIPS * KERNEL_STRIDE];
    for (mip, radius) in KERNEL_RADII.iter().copied().enumerate() {
        let radius_f = radius as f32;
        for i in 0..radius as usize {
            let x = i as f32;
            out[mip * KERNEL_STRIDE + i] =
                0.39894 * (-0.5 * x * x / (radius_f * radius_f)).exp() / radius_f;
        }
    }
    out
}

fn push_segment(
    out: &mut Vec<Vertex>,
    p0: (f32, f32),
    p1: (f32, f32),
    c0: [f32; 4],
    c1: [f32; 4],
    half_width: f32,
) {
    let dx = p1.0 - p0.0;
    let dy = p1.1 - p0.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.0001 {
        return;
    }
    let nx = -dy / len * half_width;
    let ny = dx / len * half_width;
    let ex = dx / len * half_width;
    let ey = dy / len * half_width;

    let (x0, y0) = (p0.0 - ex, p0.1 - ey);
    let (x1, y1) = (p1.0 + ex, p1.1 + ey);

    let a = [x0 + nx, y0 + ny];
    let b = [x0 - nx, y0 - ny];
    let c = [x1 + nx, y1 + ny];
    let d = [x1 - nx, y1 - ny];

    out.push(Vertex {
        position: a,
        color: c0,
    });
    out.push(Vertex {
        position: b,
        color: c0,
    });
    out.push(Vertex {
        position: c,
        color: c1,
    });
    out.push(Vertex {
        position: b,
        color: c0,
    });
    out.push(Vertex {
        position: d,
        color: c1,
    });
    out.push(Vertex {
        position: c,
        color: c1,
    });
}

/// Reparte `bands` en `n` valores cubriendo todo el espectro: si hay menos
/// valores que bandas, cada uno toma el máximo de su grupo; si hay más, se
/// interpola linealmente entre bandas vecinas.
fn resample_bands(bands: &[f32], n: usize, out: &mut Vec<f32>) {
    out.clear();
    let len = bands.len();
    if len == 0 || n == 0 {
        return;
    }
    if n <= len {
        out.extend((0..n).map(|i| {
            let lo = i * len / n;
            let hi = ((i + 1) * len / n).max(lo + 1);
            bands[lo..hi]
                .iter()
                .fold(0.0_f32, |m, &v| m.max(v))
                .clamp(0.0, 1.0)
        }));
    } else {
        out.extend((0..n).map(|i| {
            let x = i as f32 * (len - 1) as f32 / (n - 1).max(1) as f32;
            let k = (x.floor() as usize).min(len - 1);
            let next = (k + 1).min(len - 1);
            let t = x - k as f32;
            (bands[k] + (bands[next] - bands[k]) * t).clamp(0.0, 1.0)
        }));
    }
}

fn push_tri(
    out: &mut Vec<Vertex>,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    ca: [f32; 4],
    cb: [f32; 4],
    cc: [f32; 4],
) {
    out.push(Vertex {
        position: [a.0, a.1],
        color: ca,
    });
    out.push(Vertex {
        position: [b.0, b.1],
        color: cb,
    });
    out.push(Vertex {
        position: [c.0, c.1],
        color: cc,
    });
}

/// Disco relleno como abanico de triángulos.
fn push_disc(
    out: &mut Vec<Vertex>,
    center: (f32, f32),
    radius: f32,
    color: [f32; 4],
    segments: usize,
) {
    let tau = std::f32::consts::TAU;
    let point = |k: usize| {
        let ang = k as f32 / segments as f32 * tau;
        (center.0 + ang.cos() * radius, center.1 + ang.sin() * radius)
    };
    for k in 0..segments {
        push_tri(out, center, point(k), point(k + 1), color, color, color);
    }
}

#[inline]
fn scale_rgb(rgb: [f32; 3], k: f32) -> [f32; 4] {
    [rgb[0] * k, rgb[1] * k, rgb[2] * k, 1.0]
}

/// HSL → RGB en el espacio de trabajo lineal, igual que
/// `THREE.Color.setHSL` sin conversión de espacio de color.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [f32; 3] {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let l = l.clamp(0.0, 1.0);

    if s <= f32::EPSILON {
        return [l, l, l];
    }

    let q = if l <= 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    [
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    ]
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * 6.0 * (2.0 / 3.0 - t)
    } else {
        p
    }
}

/// Escala un RGB sin canal alfa.
fn scale_rgb3(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}
