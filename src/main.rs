use std::{borrow::Cow, sync::Arc};

use tokio::runtime::Runtime;
use tracing_subscriber::EnvFilter;
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowAttributes, WindowId},
};

#[repr(C)]
#[derive(Debug, PartialEq, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct VertexInput {
    pub position: [f32; 4],
    pub color: [f32; 4],
}

impl VertexInput {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
    ];

    fn desc<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

struct State {
    window: Arc<dyn Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    render_pipeline: wgpu::RenderPipeline,
    data_buffer: wgpu::Buffer,
    depth_texture_format: wgpu::TextureFormat,
    depth_texture: Option<wgpu::Texture>,
}

impl State {
    async fn new(window: Arc<dyn Window>) -> State {
        let mut backend_options = wgpu::BackendOptions::default();
        backend_options.dx12.presentation_system = wgpu::wgt::Dx12PresentationSystem::Dcomp;
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            backend_options: backend_options,
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();
        // handle to graphics card
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        // get device and queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Device"),
                required_features: wgpu::Features::empty(),
                // Make sure we use the texture resolution limits from the adapter, so we can support images the size of the swapchain.
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
                trace: wgpu::Trace::Off,
            })
            .await
            .unwrap();

        // Load the shaders from disk
        let shader = device.create_shader_module(wgpu::include_wgsl!("shaders/shader.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let swapchain_capabilities = surface.get_capabilities(&adapter);
        let swapchain_format = swapchain_capabilities.formats[0];

        let depth_texture_format = wgpu::TextureFormat::Depth24PlusStencil8;

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[VertexInput::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(swapchain_format.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: depth_texture_format,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            // depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let size = window.surface_size();

        let data_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&[
                VertexInput {
                    position: [1.0, -1.0, 0.0, 1.0],
                    color: [1.0, 0.0, 0.0, 1.0],
                },
                VertexInput {
                    position: [-1.0, -1.0, 0.0, 1.0],
                    color: [0.0, 1.0, 0.0, 1.0],
                },
                VertexInput {
                    position: [0.0, 1.0, 0.0, 1.0],
                    color: [0.0, 0.0, 1.0, 1.0],
                },
            ]),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let mut state = State {
            window,
            device,
            queue,
            size,
            surface,
            surface_format: swapchain_format,
            render_pipeline,
            data_buffer: data_buf,
            depth_texture_format,
            depth_texture: None,
        };

        // Configure surface for the first time
        state.configure_surface();

        state
    }

    fn get_window(&self) -> &dyn Window {
        self.window.as_ref()
    }

    fn configure_surface(&mut self) {
        println!("Configuring surface");
        println!("Surface size: {:?}", self.size);
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.surface_format,
                // Request compatibility with the sRGB-format texture view we‘re going to create later.
                view_formats: vec![self.surface_format.add_srgb_suffix()],
                alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                width: self.size.width,
                height: self.size.height,
                desired_maximum_frame_latency: 2,
                present_mode: wgpu::PresentMode::AutoVsync,
            },
        );

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Texture"),
            size: wgpu::Extent3d {
                width: self.size.width,
                height: self.size.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.depth_texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        self.depth_texture = Some(texture);
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        self.size = new_size;

        // reconfigure the surface
        self.configure_surface();
    }

    fn render(&mut self) {
        // Create texture view
        let surface_texture = self
            .surface
            .get_current_texture()
            .expect("failed to acquire next swapchain texture");
        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                // Without add_srgb_suffix() the image we will be working with
                // might not be "gamma correct".
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        // Renders a GREEN screen
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            // Create the renderpass which will clear the screen.
            let mut renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.3,
                            g: 0.3,
                            b: 0.3,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self
                        .depth_texture
                        .as_ref()
                        .unwrap()
                        .create_view(&Default::default()),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                // timestamp_writes: None,
                // occlusion_query_set: None,
                ..Default::default()
            });

            renderpass.set_pipeline(&self.render_pipeline);
            renderpass.set_vertex_buffer(0, self.data_buffer.slice(..));
            renderpass.draw(0..3, 0..1);
        }

        // Submit the command in the queue to execute
        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();
        surface_texture.present();
    }
}

struct App {
    state: Option<State>,
    runtime: Runtime,
}

impl Default for App {
    fn default() -> Self {
        Self {
            state: None,
            runtime: tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap(),
        }
    }
}

enum Event {}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Create window object
        let window: Arc<dyn Window> = Arc::from(
            event_loop
                .create_window(
                    WindowAttributes::default()
                        .with_transparent(true)
                        .with_title("WGPU Window"),
                )
                .unwrap(),
        );
        let state = self.runtime.block_on(State::new(window.clone()));
        self.state = Some(state);

        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent,
    ) {
        let state = self.state.as_mut().unwrap();
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; stopping");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                state.render();
                // Emits a new redraw requested event.
                state.get_window().request_redraw();
            }
            WindowEvent::SurfaceResized(size) => {
                println!("Window resized to {:?}", size);
                // Reconfigures the size of the surface. We do not re-render
                // here as this event is always followed up by redraw request.
                state.resize(size);
            }
            _ => (),
        }
    }
}

fn main() {
    let enve = EnvFilter::from_default_env();

    tracing_subscriber::fmt()
        .with_env_filter(enve)
        .with_target(false)
        .with_line_number(true)
        .with_file(true)
        .init();

    let event_loop = EventLoop::new().unwrap();

    // When the current loop iteration finishes, immediately begin a new
    // iteration regardless of whether or not new events are available to
    // process. Preferred for applications that want to render as fast as
    // possible, like games.
    event_loop.set_control_flow(ControlFlow::Poll);

    // When the current loop iteration finishes, suspend the thread until
    // another event arrives. Helps keeping CPU utilization low if nothing
    // is happening, which is preferred if the application might be idling in
    // the background.
    // event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::default();
    event_loop.run_app(&mut app).unwrap();
}
