#[cfg(target_os = "windows")]
mod windows_app {
    use app_core::{
        generate_project_with_arnis_observed, load_boundary_editor_context, set_project_boundary,
        GenerateProjectError, GenerateProjectRequest, GenerationCancellationToken, GenerationEvent,
        GenerationLogStream, GenerationResult, GenerationStage, SetProjectBoundaryRequest,
    };
    use gaode_map::{
        build_boundary_editor_html, parse_boundary_map_event, BoundaryMapConfig, BoundaryMapEvent,
    };
    use std::borrow::Cow;
    use std::error::Error;
    use std::io;
    use std::path::PathBuf;
    use std::thread;
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
    use winit::window::{Window, WindowId};
    use wry::{WebView, WebViewBuilder, WebViewBuilderExtWindows};

    #[derive(Debug)]
    enum UserEvent {
        Ipc(String),
        Generation(GenerationEvent),
        GenerationFinished {
            result: Result<GenerationResult, String>,
            cancelled: bool,
        },
    }

    struct BoundaryEditorApp {
        project_dir: PathBuf,
        arnis_executable: PathBuf,
        html: String,
        boundary_ready: bool,
        generation_token: Option<GenerationCancellationToken>,
        exit_after_generation: bool,
        proxy: EventLoopProxy<UserEvent>,
        window: Option<Window>,
        webview: Option<WebView>,
    }

    impl BoundaryEditorApp {
        fn new(
            project_dir: PathBuf,
            arnis_executable: PathBuf,
            html: String,
            boundary_ready: bool,
            proxy: EventLoopProxy<UserEvent>,
        ) -> Self {
            Self {
                project_dir,
                arnis_executable,
                html,
                boundary_ready,
                generation_token: None,
                exit_after_generation: false,
                proxy,
                window: None,
                webview: None,
            }
        }

        fn evaluate_script(&self, script: &str) {
            let Some(webview) = &self.webview else {
                return;
            };
            let _ = webview.evaluate_script(script);
        }

        fn report_to_page(&self, message: impl Into<String>) {
            let payload = serde_json::json!({ "message": message.into() });
            let script = format!("window.mcrebuildBoundaryResult({payload});");
            self.evaluate_script(&script);
        }

        fn install_generation_panel(&self) {
            self.evaluate_script(INSTALL_GENERATION_PANEL_SCRIPT);
            let script = format!(
                "window.mcrebuildInstallGenerationPanel({});",
                self.boundary_ready
            );
            self.evaluate_script(&script);
        }

        fn set_generation_boundary_ready(&self) {
            let script = format!(
                "window.mcrebuildGenerationBoundaryReady({});",
                self.boundary_ready
            );
            self.evaluate_script(&script);
        }

        fn report_generation_update(&self, payload: serde_json::Value) {
            let script = format!("window.mcrebuildGenerationUpdate({payload});");
            self.evaluate_script(&script);
        }

        fn report_generation_state(&self, status: &str, running: bool) {
            self.report_generation_update(serde_json::json!({
                "kind": "state",
                "status": status,
                "running": running,
            }));
        }

        fn report_generation_event(&self, event: GenerationEvent) {
            match event {
                GenerationEvent::Stage(stage) => {
                    self.report_generation_update(serde_json::json!({
                        "kind": "stage",
                        "stage": generation_stage_id(stage),
                        "label": generation_stage_label(stage),
                    }));
                }
                GenerationEvent::Log { stream, line } => {
                    self.report_generation_update(serde_json::json!({
                        "kind": "log",
                        "stream": match stream {
                            GenerationLogStream::Stdout => "stdout",
                            GenerationLogStream::Stderr => "stderr",
                        },
                        "line": line,
                    }));
                }
            }
        }

        fn start_generation(&mut self) {
            if self.generation_token.is_some() {
                self.report_generation_state("已有生成任务正在运行。", true);
                return;
            }
            if !self.boundary_ready {
                self.report_generation_state("请先保存有效的校园边界。", false);
                return;
            }

            let cancellation = GenerationCancellationToken::new();
            self.generation_token = Some(cancellation.clone());
            self.report_generation_state("正在启动 Arnis…", true);

            let request = GenerateProjectRequest {
                project_dir: self.project_dir.clone(),
                arnis_executable: self.arnis_executable.clone(),
            };
            let proxy = self.proxy.clone();
            thread::spawn(move || {
                let event_proxy = proxy.clone();
                let result = generate_project_with_arnis_observed(
                    request,
                    &cancellation,
                    move |event| {
                        let _ = event_proxy.send_event(UserEvent::Generation(event));
                    },
                );
                let cancelled = matches!(&result, Err(GenerateProjectError::Cancelled));
                let result = result.map_err(|error| error.to_string());
                let _ = proxy.send_event(UserEvent::GenerationFinished { result, cancelled });
            });
        }

        fn cancel_generation(&self) {
            let Some(token) = &self.generation_token else {
                self.report_generation_state("当前没有正在运行的生成任务。", false);
                return;
            };
            token.cancel();
            self.report_generation_state("正在取消生成并清理临时文件…", true);
        }

        fn request_exit(&mut self, event_loop: &ActiveEventLoop) {
            if let Some(token) = &self.generation_token {
                self.exit_after_generation = true;
                token.cancel();
                self.report_generation_state("正在取消生成，清理完成后关闭…", true);
            } else {
                event_loop.exit();
            }
        }

        fn handle_ipc(&mut self, event_loop: &ActiveEventLoop, message: String) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&message) {
                match value.get("type").and_then(serde_json::Value::as_str) {
                    Some("start_generation") => {
                        self.start_generation();
                        return;
                    }
                    Some("cancel_generation") => {
                        self.cancel_generation();
                        return;
                    }
                    _ => {}
                }
            }

            match parse_boundary_map_event(&message) {
                Ok(BoundaryMapEvent::Ready) => self.install_generation_panel(),
                Ok(BoundaryMapEvent::Cancel) => self.request_exit(event_loop),
                Ok(BoundaryMapEvent::SubmitBoundary(vertices)) => {
                    if self.generation_token.is_some() {
                        self.report_to_page(
                            "生成进行中不能修改项目边界；请先等待完成或取消生成。",
                        );
                        return;
                    }

                    let result = set_project_boundary(SetProjectBoundaryRequest {
                        project_dir: self.project_dir.clone(),
                        vertices,
                    });
                    match result {
                        Ok(project) => {
                            self.boundary_ready = project.boundary.is_some();
                            self.set_generation_boundary_ready();
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

        fn handle_generation_finished(
            &mut self,
            event_loop: &ActiveEventLoop,
            result: Result<GenerationResult, String>,
            cancelled: bool,
        ) {
            self.generation_token = None;

            match result {
                Ok(result) => {
                    self.report_generation_update(serde_json::json!({
                        "kind": "finished",
                        "status": "基础校园生成完成。",
                        "success": true,
                        "worldDir": result.world_dir.to_string_lossy(),
                    }));
                }
                Err(error) if cancelled => {
                    self.report_generation_update(serde_json::json!({
                        "kind": "finished",
                        "status": "生成已取消，临时文件已清理。",
                        "success": false,
                        "cancelled": true,
                    }));
                }
                Err(error) => {
                    self.report_generation_update(serde_json::json!({
                        "kind": "finished",
                        "status": format!("生成失败：{error}"),
                        "success": false,
                    }));
                }
            }

            if self.exit_after_generation {
                event_loop.exit();
            }
        }
    }

    impl ApplicationHandler<UserEvent> for BoundaryEditorApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.window.is_some() {
                return;
            }

            let attributes = Window::default_attributes()
                .with_title("MCRebuild · 校园重建")
                .with_inner_size(LogicalSize::new(1280.0, 840.0));
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
            match event {
                UserEvent::Ipc(message) => self.handle_ipc(event_loop, message),
                UserEvent::Generation(event) => self.report_generation_event(event),
                UserEvent::GenerationFinished { result, cancelled } => {
                    self.handle_generation_finished(event_loop, result, cancelled);
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _window_id: WindowId,
            event: WindowEvent,
        ) {
            if matches!(event, WindowEvent::CloseRequested) {
                self.request_exit(event_loop);
            }
        }
    }

    fn generation_stage_id(stage: GenerationStage) -> &'static str {
        match stage {
            GenerationStage::PreparingData => "preparing_data",
            GenerationStage::ProcessingMap => "processing_map",
            GenerationStage::GeneratingWorld => "generating_world",
            GenerationStage::SavingWorld => "saving_world",
        }
    }

    fn generation_stage_label(stage: GenerationStage) -> &'static str {
        match stage {
            GenerationStage::PreparingData => "准备地图与地形数据",
            GenerationStage::ProcessingMap => "处理校园地图对象",
            GenerationStage::GeneratingWorld => "生成 Minecraft 世界",
            GenerationStage::SavingWorld => "保存 Minecraft 世界",
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
        let boundary_ready = context.existing_boundary.is_some();
        let js_api_key = std::env::var("AMAP_JS_KEY")?;
        let security_code = std::env::var("AMAP_JS_SECURITY_CODE")?;
        let arnis_executable = std::env::var_os("ARNIS_EXECUTABLE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("arnis"));
        let html = build_boundary_editor_html(&BoundaryMapConfig {
            js_api_key,
            security_code,
            campus_display_name: context.campus_display_name,
            anchor: context.anchor,
            existing_boundary: context.existing_boundary,
        })?;

        let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
        let proxy = event_loop.create_proxy();
        let mut app = BoundaryEditorApp::new(
            project_dir,
            arnis_executable,
            html,
            boundary_ready,
            proxy,
        );
        event_loop.run_app(&mut app)?;
        Ok(())
    }

    const INSTALL_GENERATION_PANEL_SCRIPT: &str = r#"
(function(){
  if (window.mcrebuildInstallGenerationPanel) return;
  const state = { boundaryReady: false, running: false, logLines: [] };

  function send(type) {
    if (window.ipc && window.ipc.postMessage) {
      window.ipc.postMessage(JSON.stringify({ type }));
    }
  }

  function elements() {
    return {
      start: document.getElementById('generation-start'),
      cancel: document.getElementById('generation-cancel'),
      status: document.getElementById('generation-status'),
      log: document.getElementById('generation-log')
    };
  }

  function syncButtons() {
    const el = elements();
    if (!el.start || !el.cancel) return;
    el.start.disabled = state.running || !state.boundaryReady;
    el.cancel.disabled = !state.running;
  }

  function setStatus(text) {
    const el = elements();
    if (el.status) el.status.textContent = text;
  }

  function appendLog(stream, line) {
    const el = elements();
    if (!el.log) return;
    const prefix = stream === 'stderr' ? '[stderr] ' : '';
    state.logLines.push(prefix + line);
    if (state.logLines.length > 300) state.logLines.splice(0, state.logLines.length - 300);
    el.log.textContent = state.logLines.join('\n');
    el.log.scrollTop = el.log.scrollHeight;
  }

  window.mcrebuildInstallGenerationPanel = function(boundaryReady) {
    state.boundaryReady = Boolean(boundaryReady);
    if (!document.getElementById('generation-panel')) {
      const style = document.createElement('style');
      style.textContent = '#generation-panel{border-top:1px solid #e5e7eb;padding-top:14px;display:grid;gap:9px}#generation-title{font-size:14px;font-weight:650}#generation-status{font-size:12px;line-height:1.45;color:#59636e}#generation-buttons{display:grid;grid-template-columns:1fr 1fr;gap:8px}#generation-log{box-sizing:border-box;max-height:150px;min-height:76px;margin:0;padding:8px;border:1px solid #e5e7eb;border-radius:7px;background:#f8fafc;overflow:auto;white-space:pre-wrap;word-break:break-word;font:11px/1.35 ui-monospace,SFMono-Regular,Consolas,monospace;color:#374151}';
      document.head.appendChild(style);

      const section = document.createElement('section');
      section.id = 'generation-panel';
      section.innerHTML = '<div id="generation-title">基础校园生成</div><div id="generation-status">保存校园边界后即可生成。</div><div id="generation-buttons"><button id="generation-start" class="primary">开始生成</button><button id="generation-cancel" disabled>取消</button></div><pre id="generation-log"></pre>';
      const panel = document.getElementById('panel');
      const actions = panel && panel.querySelector('.actions');
      if (panel) panel.insertBefore(section, actions || null);
      document.getElementById('generation-start').addEventListener('click', function(){ send('start_generation'); });
      document.getElementById('generation-cancel').addEventListener('click', function(){ send('cancel_generation'); });
    }
    setStatus(state.boundaryReady ? '边界已就绪，可以生成基础校园。' : '请先保存有效的校园边界。');
    syncButtons();
  };

  window.mcrebuildGenerationBoundaryReady = function(ready) {
    state.boundaryReady = Boolean(ready);
    if (!state.running) {
      setStatus(state.boundaryReady ? '边界已就绪，可以生成基础校园。' : '请先保存有效的校园边界。');
    }
    syncButtons();
  };

  window.mcrebuildGenerationUpdate = function(update) {
    if (!update) return;
    if (update.kind === 'state') {
      state.running = Boolean(update.running);
      setStatus(update.status || '');
    } else if (update.kind === 'stage') {
      state.running = true;
      setStatus(update.label || '正在生成…');
    } else if (update.kind === 'log') {
      appendLog(update.stream, update.line || '');
    } else if (update.kind === 'finished') {
      state.running = false;
      setStatus(update.status || '生成结束。');
      if (update.worldDir) appendLog('stdout', 'world: ' + update.worldDir);
    }
    syncButtons();
  };
})();
"#;
}

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows_app::run()
}

#[cfg(not(target_os = "windows"))]
fn main() {}
