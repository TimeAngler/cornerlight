use std::sync::Arc;
use image::GenericImageView;
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId, WindowLevel};
use tis_renderer::renderer::WgpuRenderer;
use tis_ui::UiRenderer;
use crate::dialogue;
use log;

// 调试模式：方便测试
#[cfg(debug_assertions)]
const IDLE_RANGE: (u64, u64) = (10, 20);

// 发布模式：真实节奏
#[cfg(not(debug_assertions))]
const IDLE_RANGE: (u64, u64) = (60, 180);

struct App {
    window: Option<Arc<Window>>,
    wgpu_render: Option<WgpuRenderer>,
    ui_render: Option<UiRenderer>,
    dialogues: dialogue::DialogueSet,

    last_idle: std::time::Instant,        // 上次说话的时间
    next_idle_delay: std::time::Duration, // 下次多久后再说

    current_line: Option<String>,        // 当前显示的台词
    line_show_until: std::time::Instant, // 这句话要显示到什么时候
}

impl App {
    pub fn new() -> Self {
        Self {
            window: None,
            wgpu_render: None,
            ui_render: None,

            dialogues: dialogue::DialogueSet::load(),
            last_idle: std::time::Instant::now(),
            next_idle_delay: std::time::Duration::from_secs(1),

            current_line: None,
            line_show_until: std::time::Instant::now(),
        }
    }

    fn say(&mut self, text: &str) {
        let secs = (text.chars().count() / 3).max(3).min(8) as u64;
        // 3~8 秒，根据文本长度自适应
        self.current_line = Some(text.to_string());
        self.line_show_until = std::time::Instant::now()
            + std::time::Duration::from_secs(secs);
    }

    /// 生成 0~999_999_999 的伪随机数
    fn pseudo_random() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0)
    }

}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "windows")]
        use winit::platform::windows::WindowAttributesExtWindows;
        // 防止重复创建
        if self.window.is_some() { return; }

        // 1. 先拿到主显示器的尺寸
        let monitor = event_loop.primary_monitor().unwrap();
        let screen_size = monitor.size();
        let scale_factor = monitor.scale_factor();

        // 在 resumed 里
        let img = image::load_from_memory(include_bytes!("../assets/character.png")).unwrap();
        let (tex_w, tex_h) = (img.width(), img.height());

        // 你的立绘窗口期望的物理尺寸（根据你的图调整）
        let win_w = tex_w + 100;
        let win_h = tex_h + 100;

        // 2. 计算右下角位置（留一点边距，避免贴着屏幕边缘）
        let margin_right = 20.0;
        let margin_bottom = 60.0; // 给任务栏留点空间
        let x = screen_size.width as f64 / scale_factor - win_w as f64 - margin_right;
        let y = screen_size.height as f64 / scale_factor - win_h as f64 - margin_bottom;

        // 3. 构造 WindowAttributes
        let attributes = winit::window::WindowAttributes::default()
            .with_title("Cornerlight")                     // 会隐藏，只给任务管理器看
            .with_decorations(false)                  // 👈 去掉标题栏和边框
            .with_transparent(true)                   // 👈 保持透明
            .with_resizable(false)                    // 👈 禁止拉伸
            .with_window_level(WindowLevel::AlwaysOnTop) // 👈 永远置顶
            .with_skip_taskbar(true)
            .with_inner_size(winit::dpi::LogicalSize::new(win_w, win_h))
            .with_position(winit::dpi::LogicalPosition::new(x, y)); // 👈 定位到右下角

        let window = Arc::new(event_loop.create_window(attributes).unwrap());
        let size = window.inner_size();

        let wgpu_render = match pollster::block_on(WgpuRenderer::new(
            window.clone(),
            size.width,
            size.height,
        )) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("渲染器初始化失败: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        // 初始化 UI 渲染器
        let ui_render = UiRenderer::new(
            wgpu_render.device(),
            wgpu_render.queue(),
            wgpu_render.config().format,
            &window,
        );

        self.window = Some(window);
        self.wgpu_render = Some(wgpu_render);
        self.ui_render = Some(ui_render);

        // 打招呼
        let greeting = self.dialogues.greeting().to_string();
        self.say(&greeting);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _id: WindowId,
        event: WindowEvent
    ) {
        // 先把事件转发给 UI（egui 需要它来更新内部状态）
        if let (Some(ui), Some(window)) = (self.ui_render.as_mut(), self.window.as_ref()) {
            let _ = ui.handle_event(window, &event);
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(wgpu_renderer) = self.wgpu_render.as_mut() {
                    wgpu_renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event,.. } => {
                if event.state == ElementState::Pressed
                    && event.logical_key == Key::Named(NamedKey::Escape)
                {
                    event_loop.exit();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => {
                // 右键退出（去掉标题栏后必备的退出方式）
                event_loop.exit();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let line = self.dialogues.clicked().to_string();
                self.say(&line);

                // 重置 idle 计时，避免刚点完又自言自语
                self.last_idle = std::time::Instant::now();
                let secs = IDLE_RANGE.0
                    + Self::pseudo_random() % (IDLE_RANGE.1 - IDLE_RANGE.0 + 1);
                self.next_idle_delay = std::time::Duration::from_secs(secs);
            }
            WindowEvent::RedrawRequested => {
                if let (Some(r), Some(ui), Some(window)) = (
                    self.wgpu_render.as_mut(),
                    self.ui_render.as_mut(),
                    self.window.as_ref(),
                ) {
                    let config = r.config().clone();
                    let line = self.current_line.clone();

                    if let Err(e) = r.render_with(|encoder, view| {
                        ui.render(
                            encoder, view, window,
                            config.width, config.height,
                            line.as_deref(),
                        );
                    }) {
                        if e.to_string().contains("失去device") {
                            eprintln!("设备丢失，退出");
                            event_loop.exit();
                        } else {
                            eprintln!("渲染错误: {:?}", e);
                        }
                    }
                }

                if let Some(window) = self.window.as_ref() {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 台词到点消失
        if self.current_line.is_some() && std::time::Instant::now() >= self.line_show_until {
            self.current_line = None;
            if let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }

        // idle 说话
        if self.last_idle.elapsed() >= self.next_idle_delay {
            let line = self.dialogues.idle().to_string();
            self.say(&line);

            self.last_idle = std::time::Instant::now();
            let secs = IDLE_RANGE.0
                + Self::pseudo_random() % (IDLE_RANGE.1 - IDLE_RANGE.0 + 1);
            self.next_idle_delay = std::time::Duration::from_secs(secs);
        }

        // 让事件循环下次在"台词消失"或"该说下一句"的时刻醒来
        let next_wake = (self.line_show_until).min(self.last_idle + self.next_idle_delay);
        event_loop.set_control_flow(
            winit::event_loop::ControlFlow::WaitUntil(next_wake)
        );
    }
}

pub fn run() -> anyhow::Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info")
    )
        .format_timestamp(None)
        .init();

    let event_loop = EventLoop::new()?;
    let mut app = App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
