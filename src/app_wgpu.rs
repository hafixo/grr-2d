use crate::{GpuData, Viewport};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent, DeviceEvent, ElementState, MouseScrollDelta};
use winit::event_loop::ControlFlow;
use std::error::Error;
use wgpu::vertex_attr_array;
use zerocopy::{AsBytes, FromBytes};

struct FrameTime(f32);

impl FrameTime {
    pub fn update(&mut self, t: f32) {
        self.0 = self.0 * 0.95 + 0.05 * t;
    }
}

#[repr(C)]
#[derive(Copy, Clone, AsBytes)]
struct Locals {
    viewport: [f32; 4],
    screen_dim: [f32; 2],
    num_primitives: u32,
    _pad: u32,
}

pub unsafe fn run_wgpu(name: &'static str, gpu_data: GpuData) -> Result<(), Box<dyn Error>> {
    let mut event_loop = winit::event_loop::EventLoop::new();
    let wb = winit::window::WindowBuilder::new()
        .with_title(name)
        .with_inner_size(LogicalSize {
            width: 1240.0,
            height: 700.0,
        });
    let window = wb
        .build(&event_loop)?;

    let size = window.inner_size();

    let surface = wgpu::Surface::create(&window);

    let adapter = wgpu::Adapter::request(
        &wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::Default,
        },
        wgpu::BackendBit::PRIMARY,
    ).unwrap();

    let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
        extensions: wgpu::Extensions {
            anisotropic_filtering: false,
        },
        limits: wgpu::Limits::default(),
    });

    let mut sc_desc = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        width: size.width,
        height: size.height,
        present_mode: wgpu::PresentMode::Vsync,
    };
    let mut swap_chain = device.create_swap_chain(&surface, &sc_desc);

    let vs_module =
        device.create_shader_module(&wgpu::read_spirv(std::io::Cursor::new(&include_bytes!("../assets/lanka.vs.spv")[..])).unwrap());
    let fs_module =
        device.create_shader_module(&wgpu::read_spirv(std::io::Cursor::new(&include_bytes!("../assets/lanka.fs.spv")[..])).unwrap());

    let bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { bindings: &[
            wgpu::BindGroupLayoutBinding {
                binding: 0,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::StorageBuffer { dynamic: false, readonly: true },
            },
            wgpu::BindGroupLayoutBinding {
                binding: 1,
                visibility: wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::StorageBuffer { dynamic: false, readonly: true },
            },
            wgpu::BindGroupLayoutBinding {
                binding: 2,
                visibility: wgpu::ShaderStage::VERTEX | wgpu::ShaderStage::FRAGMENT,
                ty: wgpu::BindingType::UniformBuffer { dynamic: false },
            },
        ] });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        bind_group_layouts: &[&bind_group_layout],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        layout: &pipeline_layout,
        vertex_stage: wgpu::ProgrammableStageDescriptor {
            module: &vs_module,
            entry_point: "main",
        },
        fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
            module: &fs_module,
            entry_point: "main",
        }),
        rasterization_state: Some(wgpu::RasterizationStateDescriptor {
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: wgpu::CullMode::None,
            depth_bias: 0,
            depth_bias_slope_scale: 0.0,
            depth_bias_clamp: 0.0,
        }),
        primitive_topology: wgpu::PrimitiveTopology::TriangleList,
        color_states: &[wgpu::ColorStateDescriptor {
            format: wgpu::TextureFormat::Bgra8UnormSrgb,
            color_blend: wgpu::BlendDescriptor {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha_blend: wgpu::BlendDescriptor {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            write_mask: wgpu::ColorWrite::ALL,
        }],
        depth_stencil_state: None,
        index_format: wgpu::IndexFormat::Uint16,
        vertex_buffers: &[
            wgpu::VertexBufferDescriptor {
                stride: (std::mem::size_of::<f32>() * 4) as wgpu::BufferAddress,
                step_mode: wgpu::InputStepMode::Vertex,
                attributes: &vertex_attr_array![0 => Float2, 1 => Float2],
            },
            wgpu::VertexBufferDescriptor {
                stride: (std::mem::size_of::<u32>() * 3) as wgpu::BufferAddress,
                step_mode: wgpu::InputStepMode::Vertex,
                attributes: &vertex_attr_array![2 => Uint3],
            }
        ],
        sample_count: 1,
        sample_mask: !0,
        alpha_to_coverage_enabled: false,
    });

    let gpu_vertices = device.create_buffer_with_data(
        gpu_data.vertices.as_bytes(),
        wgpu::BufferUsage::STORAGE_READ,
    );

    let gpu_bbox = device.create_buffer_with_data(
        gpu_data.bbox.as_bytes(),
        wgpu::BufferUsage::VERTEX,
    );

    let gpu_primitives = device.create_buffer_with_data(
        gpu_data.primitives.as_bytes(),
        wgpu::BufferUsage::STORAGE_READ,
    );

    let gpu_curve_ranges = device.create_buffer_with_data(
        gpu_data.curve_ranges.as_bytes(),
        wgpu::BufferUsage::VERTEX,
    );

    let mut viewport = Viewport {
        position: (0.0, 0.0),
        scaling_y: size.height as _,
        aspect_ratio: (size.width as f32 / size.height as f32),
    };

    let locals_dummy = Locals {
        viewport: viewport.get_rect(),
        screen_dim: [size.width as f32, size.height as f32],
        num_primitives: gpu_data.primitives.len() as _,
        _pad: 0,
    };
    let locals = device.create_buffer_with_data(locals_dummy.as_bytes(), wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST);

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        bindings: &[
            wgpu::Binding {
                binding: 0,
                resource: wgpu::BindingResource::Buffer {
                    buffer: &gpu_vertices,
                    range: 0 .. gpu_data.vertices.as_bytes().len() as _,
                },
            },
            wgpu::Binding {
                binding: 1,
                resource: wgpu::BindingResource::Buffer {
                    buffer: &gpu_primitives,
                    range: 0 .. gpu_data.primitives.as_bytes().len() as _,
                },
            },
            wgpu::Binding {
                binding: 2,
                resource: wgpu::BindingResource::Buffer {
                    buffer: &locals,
                    range: 0 .. 32,
                },
            }
        ],
    });

    let mut time_last = std::time::Instant::now();
    let mut avg_frametime_cpu = FrameTime(0.0);
    let mut avg_frametime_gpu = FrameTime(0.0);


    // let query = [
    //     grr.create_query(grr::QueryType::Timestamp),
    //     grr.create_query(grr::QueryType::Timestamp),
    // ];

    let mut mouse1 = ElementState::Released;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;
        match event {
            Event::MainEventsCleared => window.request_redraw(),
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                WindowEvent::Resized(size) => {
                    sc_desc.width = size.width;
                    sc_desc.height = size.height;
                    swap_chain = device.create_swap_chain(&surface, &sc_desc);
                }
                _ => (),
            },
            Event::DeviceEvent {
                event: DeviceEvent::MouseMotion { delta },
                ..
            } => {
                if mouse1 == ElementState::Pressed {
                    let scale = viewport.get_scale();
                    viewport.position.0 -= scale.0 * (delta.0 / size.width as f64) as f32;
                    viewport.position.1 += scale.1 * (delta.1 / size.height as f64) as f32;
                }
            }
            Event::DeviceEvent {
                event:
                    DeviceEvent::MouseWheel {
                        delta: MouseScrollDelta::LineDelta(_, delta),
                    },
                ..
            } => {
                viewport.scaling_y *= (delta * -0.1).exp();
            }
            Event::DeviceEvent {
                event: DeviceEvent::Button { state, .. },
                ..
            } => {
                mouse1 = state;
            }
            Event::RedrawRequested(_) => {
                // timing
                let time_now = std::time::Instant::now();
                let elapsed = time_now.duration_since(time_last).as_micros() as f32 / 1_000_000.0;
                time_last = time_now;
                avg_frametime_cpu.update(elapsed);
                window.set_title(&format!(
                    "grr-2d :: frame: cpu: {:.2} ms | gpu: {:.2} ms",
                    avg_frametime_cpu.0 * 1000.0,
                    avg_frametime_gpu.0 * 1000.0,
                ));

                let frame = swap_chain
                    .get_next_texture()
                    .expect("Timeout when acquiring next swap chain texture");

                let locals_dummy = Locals {
                    viewport: viewport.get_rect(),
                    screen_dim: [size.width as f32, size.height as f32],
                    num_primitives: gpu_data.primitives.len() as _,
                    _pad: 0,
                };
                let temp_buf =
                    device.create_buffer_with_data(locals_dummy.as_bytes(), wgpu::BufferUsage::COPY_SRC);


                let mut encoder =
                    device.create_command_encoder(&wgpu::CommandEncoderDescriptor { todo: 0 });
                {
                    encoder.copy_buffer_to_buffer(&temp_buf, 0, &locals, 0, std::mem::size_of::<Locals>() as _);
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                            attachment: &frame.view,
                            resolve_target: None,
                            load_op: wgpu::LoadOp::Clear,
                            store_op: wgpu::StoreOp::Store,
                            clear_color: wgpu::Color {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            },
                        }],
                        depth_stencil_attachment: None,
                    });
                    rpass.set_pipeline(&render_pipeline);
                    rpass.set_bind_group(0, &bind_group, &[]);
                    rpass.set_vertex_buffers(0, &[(&gpu_bbox, 0), (&gpu_curve_ranges, 0)]);

                    let num_vertices = gpu_data.bbox.len() as u32 / 4;
                    rpass.draw(0 .. num_vertices, 0 .. 1);
                }

                queue.submit(&[encoder.finish()]);
            }
            _ => (),
        }
    });
}
