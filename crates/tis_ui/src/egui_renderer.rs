use std::sync::Arc;
use egui::{Context, FontData, FontDefinitions, Ui};
use egui_wgpu::Renderer;
use egui_winit::State;
use winit::window::Window;

pub struct EguiRenderer {
    pub context: Context,
    winit_state: State,
    renderer: Renderer,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl EguiRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        output_color_format: wgpu::TextureFormat,
        window: &Window,
    ) -> Self {
        let egui_context = Context::default();
        setup_cjk_fonts(&egui_context);

        let egui_winit_state = State::new(
            egui_context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,          // theme: Option<egui::Theme>
            None,          // max_texture_side: Option<usize>
        );

        // device: &Device,
        // output_color_format: TextureFormat,
        // options: RendererOptions,
        let egui_renderer = Renderer::new(
            device,
            output_color_format,
            egui_wgpu::RendererOptions {
                msaa_samples: 0,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            }
        );

        Self {
            context: egui_context,
            winit_state: egui_winit_state,
            renderer: egui_renderer,
            device: device.clone(),
            queue: queue.clone(),
        }
    }

    /// 转发 winit 事件给 egui。
    pub fn handle_event(&mut self, window: &Window, event: &winit::event::WindowEvent)
    -> bool {
        let response = self.winit_state
            .on_window_event(window, event);
        response.consumed
    }

    pub fn paint(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        window: &Window,
        surface_width: u32,
        surface_height: u32,
        bubble_text: Option<&str>,
    ) {
        let raw_input = self.winit_state.take_egui_input(window);
        let text = bubble_text.map(String::from);
        let full_output = self.context.run_ui(raw_input, |ui| {
            if let Some(t) = text.as_deref() {
                let screen_rect = ui.max_rect();

                // ① 定字号、定颜色
                let font = egui::FontId::proportional(18.0);
                let text_color = egui::Color32::WHITE;

                let max_w = screen_rect.width() - 40.0;
                let pad_x = 20.0;
                let pad_y = 12.0;

                // 文字能占的最大宽度（去掉左右内边距）
                let wrap_w = max_w - pad_x * 2.0;

                // ② 量一下文字实际占多大
                let galley = ui.painter().layout(
                    t.to_string(),
                    font,
                    text_color,
                    wrap_w
                );

                let text_size = galley.size();

                // ④ 气泡尺寸 = 文字尺寸 + 内边距，最宽不超过屏幕 - 40
                let bubble_w = (text_size.x + pad_x * 2.0).min(max_w);
                let bubble_h = text_size.y + pad_y * 2.0;

                // ⑤ 位置：顶部居中，距顶 20px
                let center = egui::pos2(
                    screen_rect.center().x,
                    screen_rect.min.y + 20.0 + bubble_h * 0.5,
                );
                let bubble_rect = egui::Rect::from_center_size(
                    center,
                    egui::vec2(bubble_w, bubble_h),
                );

                // ⑥ 画背景，圆角 12
                ui.painter().rect_filled(
                    bubble_rect,
                    12.0,
                    egui::Color32::from_rgba_unmultiplied(30, 30, 45, 220),
                );

                // ⑦ 画文字：左上角定位
                let text_pos = bubble_rect.center() - text_size * 0.5;
                ui.painter().galley(text_pos, galley, text_color);
            }
        });

        let egui::FullOutput {
            shapes,
            mut textures_delta,
            platform_output,
            ..
        } = full_output;


        let clipped_primitives = self.context.tessellate(
            shapes,
            window.scale_factor() as f32,
        );

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_width, surface_height],
            pixels_per_point: window.scale_factor() as f32,
        };

        // 更新字体图集等纹理
        for (id, deltas) in &textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, delta);
            }
        }

        // 更新顶点/索引缓冲
        let user_cmd_bufs = self.renderer.update_buffers(
            &self.device,
            &self.queue,
            encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        // 开始 egui 的渲染 pass
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // 加载已有场景内容
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
                multiview_mask: None,
            });

            self.renderer
                .render(&mut pass.forget_lifetime(), &clipped_primitives, &screen_descriptor);
        }

        self.queue.submit(user_cmd_bufs);

        for id in &textures_delta.free {
            self.renderer.free_texture(id);
        }

        textures_delta.clear();
    }

}


fn setup_cjk_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    // 按优先级加载字体，从系统路径加载
    // 这里的路径需要根据操作系统判断，以下以 Windows 为例
    #[cfg(target_os = "windows")]
    const FONT_PATH: &str = "C:\\Windows\\Fonts\\msyh.ttc"; // 微软雅黑

    if let Ok(font_data) = std::fs::read(FONT_PATH) {
        fonts.font_data.insert(
            "cjk_font".to_owned(),
            Arc::new(FontData::from_owned(font_data)),
        );
        // 将中文字体插入到默认字体列表的最前面
        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap()
            .insert(0, "cjk_font".to_owned());
        fonts.families.get_mut(&egui::FontFamily::Monospace).unwrap()
            .insert(0, "cjk_font".to_owned());
    }
    ctx.set_fonts(fonts);

}