#[cfg(target_os = "windows")]
mod windows_app {
    use app_core::{load_boundary_editor_context, set_project_boundary, SetProjectBoundaryRequest};
    use gaode_map::{
        build_boundary_editor_html, parse_boundary_map_event, BoundaryMapConfig, BoundaryMapEvent,
    };
    use std::borrow::Cow;
    use std::error::Error;
    use std::io;
    use std::path::PathBuf;
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
    use winit::window::{Window, WindowId};
    use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows};

    #[derive(Debug)]
    enum UserEvent {
        Ipc(String),
    }

    struct BoundaryEditorApp {
        project_dir: PathBuf,
        html: String,
        proxy: EventLoopProxy<UserEvent>,
        window: Option<Window>,
        webview: Option<WebView>,
    }

    impl BoundaryEditorApp {
        fn new(project_dir: PathBuf, html: String, proxy: EventLoopProxy<UserEvent>) -> Self {
            Self {
                project_dir,
                html,
                proxy,
                window: None,
                webview: None,
            }
        }

        fn report_to_page(&self, message: impl Into<String>) {
            let Some(webview) = &self.webview else {
                return;
            };
            let payload = serde_json::json!({ "message": message.into() });
            let script = format!("window.mcrebuildBoundaryResult({payload});");
            let _ = webview.evaluate_script(&script);
        }

        fn handle_ipc(&mut self, event_loop: &ActiveEventLoop, message: String) {
            match parse_boundary_map_event(&message) {
                Ok(BoundaryMapEvent::Ready) => {}
                Ok(BoundaryMapEvent::Cancel) => event_loop.exit(),
                Ok(BoundaryMapEvent::SubmitBoundary(vertices)) => {
                    let result = set_project_boundary(SetProjectBoundaryRequest {
                        project_dir: self.project_dir.clone(),
                        vertices,
                    });
                    match result {
                        Ok(project) => {
                            let area_m2 = project
                                .boundary
                                .as_ref()
                                .map(|boundary| boundary.area_m2())
                                .unwrap_or_default();
                            self.report_to_page(format!("边界已保存，面积约 {:.0} m²。", area_m2));
                        }
                        Err(error) => self.report_to_page(format!("边界无效：{error}")),
                    }
                }
                Err(error) => self.report_to_page(format!("地图消息无效：{error}")),
            }
        }
    }

    impl ApplicationHandler<UserEvent> for BoundaryEditorApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }

            let attributes = Window::default_attributes()
                .with_title("MCRebuild · 校园边界")
                .with_inner_size(LogicalSize::new(1280.0, 800.0));
            let window = event_loop
                .create_window(attributes)
                .expect("create MCRebuild window");

            let page = self.html.clone().into_bytes();
            let proxy = self.proxy.clone();
            let webview = WebViewBuilder::new()
                .with_custom_protocol("mcrebuild".into(), move |_webview_id, _request| {
                    wry::http::Response::builder()
                        .header("Content-Type", "text/html; charset=utf-8")
                        .body(Cow::Owned(page.clone()))
                        .expect("build local page response")
                })
                .with_https_scheme(true)
                .with_ipc_handler(move |request| {
                    let _ = proxy.send_event(UserEvent::Ipc(request.body().clone()));
                })
                .with_url("mcrebuild://localhost/")
                .build(&window)
                .expect("create MCRebuild webview");

            self.window = Some(window);
            self.webview = Some(webview);
        }

        fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
            let UserEvent::Ipc(message) = event;
            self.handle_ipc(event_loop, message);
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
        }
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let project_dir = std::env::args_os()
            .nth(1)
            .map(PathBuf::from)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "usage: mcrebuild-desktop <project_dir>",
                )
            })?;
        let context = load_boundary_editor_context(&project_dir)?;
        let js_api_key = std::env::var("AMAP_JS_KEY")?;
        let security_code = std::env::var("AMAP_JS_SECURITY_CODE")?;
        let html = build_boundary_editor_html(&BoundaryMapConfig {
            js_api_key,
            security_code,
            campus_display_name: context.campus_display_name,
            anchor: context.anchor,
            existing_boundary: context.existing_boundary,
        })?;

        let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
        let proxy = event_loop.create_proxy();
        let mut app = BoundaryEditorApp::new(project_dir, html, proxy);
        event_loop.run_app(&mut app)?;
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_app::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {}
