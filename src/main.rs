use blockidle_app::{
    AppPaths, ClientController, NavigationDecision, PlatformEffect, TrayAction, TrayMenuState,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Fullscreen, Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use wry::{WebContext, WebView, WebViewBuilder};

#[derive(Debug)]
enum UserEvent {
    IpcMessage(String),
    TrayAction(TrayAction),
    Effects(Vec<PlatformEffect>),
}

struct SystemTray {
    _tray_icon: TrayIcon,
    toggle_visibility_item: MenuItem,
    toggle_mute_item: MenuItem,
}

impl SystemTray {
    fn sync(&self, state: TrayMenuState) {
        self.toggle_visibility_item.set_text(if state.window_visible {
            "Hide to Tray"
        } else {
            "Show Blockidle"
        });
        self.toggle_mute_item.set_text(if state.audio_muted {
            "Unmute Audio"
        } else {
            "Mute Audio"
        });
    }
}

fn build_tray_icon_rgba() -> Option<Icon> {
    let width = 32u32;
    let height = 32u32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let border = x < 3 || x >= width - 3 || y < 3 || y >= height - 3;
            if border {
                rgba.extend_from_slice(&[34, 197, 94, 255]);
            } else {
                rgba.extend_from_slice(&[20, 83, 45, 255]);
            }
        }
    }
    Icon::from_rgba(rgba, width, height).ok()
}

fn build_system_tray(
    initial_state: TrayMenuState,
    proxy: EventLoopProxy<UserEvent>,
) -> Option<SystemTray> {
    let menu = Menu::new();
    let toggle_visibility_item = MenuItem::new(
        if initial_state.window_visible {
            "Hide to Tray"
        } else {
            "Show Blockidle"
        },
        true,
        None,
    );
    let toggle_mute_item = MenuItem::new(
        if initial_state.audio_muted {
            "Unmute Audio"
        } else {
            "Mute Audio"
        },
        true,
        None,
    );
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Quit Blockidle", true, None);

    let _ = menu.append(&toggle_visibility_item);
    let _ = menu.append(&toggle_mute_item);
    let _ = menu.append(&separator);
    let _ = menu.append(&quit_item);

    let vis_id = toggle_visibility_item.id().clone();
    let mute_id = toggle_mute_item.id().clone();
    let quit_id = quit_item.id().clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == vis_id {
            let _ = proxy.send_event(UserEvent::TrayAction(TrayAction::ToggleWindowVisibility));
        } else if event.id == mute_id {
            let _ = proxy.send_event(UserEvent::TrayAction(TrayAction::ToggleMute));
        } else if event.id == quit_id {
            let _ = proxy.send_event(UserEvent::TrayAction(TrayAction::Quit));
        }
    }));

    let mut builder = TrayIconBuilder::new()
        .with_tooltip("Blockidle")
        .with_menu(Box::new(menu));
    if let Some(icon) = build_tray_icon_rgba() {
        builder = builder.with_icon(icon);
    }

    let tray_icon = builder.build().ok()?;
    Some(SystemTray {
        _tray_icon: tray_icon,
        toggle_visibility_item,
        toggle_mute_item,
    })
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
    tray: Option<&SystemTray>,
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
                if visible {
                    window.set_focus();
                }
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
            PlatformEffect::UpdateTrayMenu(state) => {
                if let Some(tray) = tray {
                    tray.sync(state);
                }
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

    let (width, height, fullscreen, data_dir, initial_tray_state) = {
        let guard = controller.lock().expect("controller lock");
        let settings = guard.settings();
        (
            settings.window_width,
            settings.window_height,
            settings.fullscreen,
            guard.data_dir().to_path_buf(),
            guard.tray_menu_state(),
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

    let tray = build_system_tray(initial_tray_state, proxy.clone());

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
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
                WindowEvent::Focused(focused) => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_focus_changed(focused);
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
                WindowEvent::Resized(physical_size) => {
                    let is_minimized = window.is_minimized();
                    let effects = if is_minimized {
                        controller
                            .lock()
                            .expect("controller lock")
                            .handle_minimized_changed(true)
                    } else {
                        let scale = window.scale_factor();
                        let logical = physical_size.to_logical::<u32>(scale);
                        controller
                            .lock()
                            .expect("controller lock")
                            .handle_window_resized(logical.width, logical.height)
                    };
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
                _ => {}
            },
            Event::UserEvent(user_event) => match user_event {
                UserEvent::IpcMessage(payload) => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_ipc_message(&payload);
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
                UserEvent::TrayAction(action) => {
                    let effects = controller
                        .lock()
                        .expect("controller lock")
                        .handle_tray_action(action);
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
                UserEvent::Effects(effects) => {
                    execute_effects(effects, &window, &webview, tray.as_ref(), control_flow);
                }
            },
            _ => {}
        }
    });
}
