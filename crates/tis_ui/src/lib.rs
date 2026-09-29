mod egui_renderer;

use winit::window::Window;

pub struct UiRenderer {
    inner: egui_renderer::EguiRenderer,
}

impl UiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        window: &Window,
    ) -> Self {
        Self {
            inner: egui_renderer::EguiRenderer::new(
                device, queue, surface_format, window,
            ),
        }
    }

    /// 转发 winit 事件给 UI。返回 true 表示 UI 消费了这次事件。
    /// 主 crate 可以用它来实现"点 UI 时不要让点击穿透到场景"。
    pub fn handle_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        self.inner.handle_event(window, event)
    }

    /// 渲染一帧 UI。
    /// `bubble_text`: 台词内容，None 表示不显示气泡。
    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        window: &Window,
        surface_width: u32,
        surface_height: u32,
        bubble_text: Option<&str>,
    ) {
        self.inner.paint(
            encoder,
            target_view,
            window,
            surface_width,
            surface_height,
            bubble_text,
        );
    }

}


