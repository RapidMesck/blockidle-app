use blockidle_app::{AppPaths, ClientController, NavigationDecision, PlatformEffect};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Fullscreen, Window, WindowBuilder};
use wry::{WebContext, WebView, WebViewBuilder};

#[derive(Debug)]
enum UserEvent {
    IpcMessage(String),
    Effects(Vec<PlatformEffect>),
}

fn resolve_paths() -> AppPaths {
    AppPaths::resolve().unwrap_or_else(|| AppPaths {
        config_dir: PathBuf::from(".blockidle/config"),
        data_dir: PathBuf::from(".blockidle/data"),
    })
}

fn execute_effects(
    effects: Vec<PlatformEffect>,
    window: &Window,
    webview: &WebView,
    control_flow: &mut ControlFlow,
) {
    for effect in effects {
        match effect {
            PlatformEffect::OpenExternalBrowser(url) => {
                let _ = open::that_detached(url);
            }
            PlatformEffect::SyncWebViewState(script) => {
                let _ = webview.evaluate_script(&script);
            }
            PlatformEffect::SetWindowVisible(visible) => {
                window.set_visible(visible);
            }
            PlatformEffect::SetFullscreen(fullscreen) => {
                if fullscreen {
                    window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                } else {
                    window.set_fullscreen(None);
                }
            }
            PlatformEffect::ReloadGameView => {
                let _ = webview.evaluate_script("window.location.reload();");
            }
            PlatformEffect::ExitApp => {
                *control_flow = ControlFlow::Exit;
            }
        }
    }
}

fn build_game_view(
    window: &Window,
    web_context: &mut WebContext,
    controller: &Arc<Mutex<ClientController>>,
    proxy: EventLoopProxy<UserEvent>,
) -> wry::Result<WebView> {
    let (initial_url, init_script) = {
        let guard = controller.lock().expect("controller lock");
        (
            guard.game_view_url().to_string(),
            guard.initialization_script(),
        )
    };

    let ipc_proxy = proxy.clone();
    let nav_proxy = proxy.clone();
    let nav_controller = Arc::clone(controller);
    let popup_proxy = proxy;
    let popup_controller = Arc::clone(controller);

    let builder = WebViewBuilder::with_web_context(web_context)
        .with_url(&initial_url)
        .with_initialization_script(&init_script)
        .with_clipboard(true)
        .with_ipc_handler(move |request| {
            let _ = ipc_proxy.send_event(UserEvent::IpcMessage(request.body().to_string()));
        })
        .with_navigation_handler(move |uri| {
            let (decision, effects) = nav_controller
                .lock()
                .expect("controller lock")
                .handle_navigation_request(&uri);
            if !effects.is_empty() {
                let _ = nav_proxy.send_event(UserEvent::Effects(effects));
            }
            decision == NavigationDecision::AllowInGameView
        })
        .with_new_window_req_handler(move |uri| {
            let (decision, effects) = popup_controller
                .lock()
                .expect("controller lock")
                .handle_new_window_request(&uri);
            if !effects.is_empty() {
                let _ = popup_proxy.send_event(UserEvent::Effects(effects));
            }
            decision == NavigationDecision::AllowInGameView
        });

    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .expect("GTK default_vbox required on Linux");
        builder.build_gtk(vbox)
    }

    #[cfg(not(target_os = "linux"))]
    {
        builder.build(window)
    }
}

fn main() -> wry::Result<()> {
    let paths = resolve_paths();
    let _ = std::fs::create_dir_all(&paths.config_dir);
    let _ = std::fs::create_dir_all(&paths.data_dir);

    let controller = Arc::new(Mutex::new(ClientController::with_paths(
        &paths.config_dir,
        &paths.data_dir,
    )));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (width, height, fullscreen, data_dir) = {
        let guard = controller.lock().expect("controller lock");
        let settings = guard.settings();
        (
            settings.window_width,
            settings.window_height,
            settings.fullscreen,
            guard.data_dir().to_path_buf(),
        )
    };

    let window = WindowBuilder::new()
        .with_title("Blockidle")
        .with_inner_size(LogicalSize::new(width, height))
        .with_fullscreen(if fullscreen {
            Some(Fullscreen::Borderless(None))
        } else {
            None
        })
        .build(&event_loop)
        .expect("failed to build Client Shell window");

    let mut web_context = WebContext::new(Some(data_dir));
    let webview = build_game_view(&window, &mut web_context, &controller, proxy)?;

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_close_requested();
                    execute_effects(effects, &window, &webview, control_flow);
                }
                WindowEvent::Focused(focused) => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_focus_changed(focused);
                    execute_effects(effects, &window, &webview, control_flow);
                }
                WindowEvent::Resized(physical_size) => {
                    let scale = window.scale_factor();
                    let logical = physical_size.to_logical::<u32>(scale);
                    controller
                        .lock()
                        .expect("controller lock")
                        .handle_window_resized(logical.width, logical.height);
                }
                _ => {}
            },
            Event::UserEvent(user_event) => match user_event {
                UserEvent::IpcMessage(payload) => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_ipc_message(&payload);
                    execute_effects(effects, &window, &webview, control_flow);
                }
                UserEvent::Effects(effects) => {
                    execute_effects(effects, &window, &webview, control_flow);
                }
            },
            _ => {}
        }
    });
}
