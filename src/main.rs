#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use blockidle_app::{
    AppPaths, ClientController, NavigationDecision, PlatformEffect, TrayAction, TrayMenuState,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Fullscreen, Icon as WindowIcon, Window, WindowBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon as TrayIconImage, TrayIcon, TrayIconBuilder};
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

fn load_app_window_icon() -> Option<WindowIcon> {
    const RGBA: &[u8] = include_bytes!("../assets/icons/icon_128.rgba");
    WindowIcon::from_rgba(RGBA.to_vec(), 128, 128).ok()
}

fn load_app_tray_icon() -> Option<TrayIconImage> {
    const RGBA: &[u8] = include_bytes!("../assets/icons/icon_32.rgba");
    TrayIconImage::from_rgba(RGBA.to_vec(), 32, 32).ok()
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
    if let Some(icon) = load_app_tray_icon() {
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
                    window.set_minimized(false);
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

#[cfg(target_os = "linux")]
mod linux_runtime {
    use blockidle_app::ClientController;
    use std::ffi::{c_char, c_void};
    use std::sync::Once;

    const G_LOG_FLAG_RECURSION: u32 = 1 << 0;
    const G_LOG_FLAG_FATAL: u32 = 1 << 1;
    const G_LOG_LEVEL_WARNING: u32 = 1 << 4;
    const GTK_STYLE_PROVIDER_PRIORITY_USER: u32 = 800;

    #[repr(C)]
    struct GList {
        data: *mut c_void,
        next: *mut GList,
        prev: *mut GList,
    }

    unsafe extern "C" {
        fn g_log_set_handler(
            log_domain: *const c_char,
            log_levels: u32,
            log_func: Option<
                unsafe extern "C" fn(*const c_char, u32, *const c_char, *mut c_void),
            >,
            user_data: *mut c_void,
        ) -> u32;
        fn gtk_settings_get_default() -> *mut c_void;
        fn g_object_set(object: *mut c_void, first_property_name: *const c_char, ...);
        fn gdk_screen_get_default() -> *mut c_void;
        fn gtk_css_provider_new() -> *mut c_void;
        fn gtk_css_provider_load_from_data(
            provider: *mut c_void,
            data: *const c_char,
            length: isize,
            error: *mut *mut c_void,
        ) -> i32;
        fn gtk_style_context_add_provider_for_screen(
            screen: *mut c_void,
            provider: *mut c_void,
            priority: u32,
        );
        fn gtk_window_list_toplevels() -> *mut GList;
        fn g_list_free(list: *mut GList);
        fn gtk_window_get_titlebar(window: *mut c_void) -> *mut c_void;
        fn gtk_bin_get_child(bin: *mut c_void) -> *mut c_void;
        fn gtk_header_bar_set_has_subtitle(bar: *mut c_void, setting: i32);
        fn gtk_widget_get_style_context(widget: *mut c_void) -> *mut c_void;
        fn gtk_style_context_add_class(context: *mut c_void, class_name: *const c_char);
        fn gtk_widget_queue_resize(widget: *mut c_void);
    }

    unsafe extern "C" fn ignore_ayatana_warning(
        _domain: *const c_char,
        _level: u32,
        _message: *const c_char,
        _user_data: *mut c_void,
    ) {
    }

    pub fn configure_pre_init(controller: &ClientController) {
        // SAFETY: Called on the main thread before GTK/WebKit or any worker threads are spawned.
        unsafe {
            if std::env::var_os("FONTCONFIG_FILE").is_none()
                && let Ok(fonts_conf) = controller.ensure_fontconfig_file()
            {
                std::env::set_var("FONTCONFIG_FILE", fonts_conf);
            }
            g_log_set_handler(
                c"libayatana-appindicator".as_ptr(),
                G_LOG_LEVEL_WARNING | G_LOG_FLAG_RECURSION | G_LOG_FLAG_FATAL,
                Some(ignore_ayatana_warning),
                std::ptr::null_mut(),
            );
        }
    }

    pub fn apply_gtk_settings() {
        static CSS_INIT: Once = Once::new();
        unsafe {
            let settings = gtk_settings_get_default();
            if !settings.is_null() {
                g_object_set(
                    settings,
                    c"gtk-xft-hinting".as_ptr(),
                    1i32,
                    c"gtk-xft-hintstyle".as_ptr(),
                    c"hintslight".as_ptr(),
                    c"gtk-xft-antialias".as_ptr(),
                    1i32,
                    c"gtk-xft-rgba".as_ptr(),
                    c"none".as_ptr(),
                    std::ptr::null::<c_char>(),
                );
            }

            CSS_INIT.call_once(|| {
                let screen = gdk_screen_get_default();
                if !screen.is_null() {
                    let provider = gtk_css_provider_new();
                    if !provider.is_null() {
                        let css = c"
                            headerbar, .titlebar, window headerbar, window .titlebar,
                            window headerbar.titlebar, dialog headerbar.titlebar,
                            headerbar.default-decoration, .titlebar.default-decoration {
                                min-height: 0px;
                                padding-top: 1px;
                                padding-bottom: 1px;
                                padding-left: 6px;
                                padding-right: 6px;
                                margin: 0px;
                            }
                            headerbar *, .titlebar * {
                                margin-top: 0px;
                                margin-bottom: 0px;
                                min-height: 0px;
                            }
                            headerbar entry, headerbar spinbutton, headerbar separator,
                            headerbar button, headerbar button.titlebutton, .titlebar button.titlebutton,
                            window headerbar.default-decoration button.titlebutton,
                            window .titlebar.default-decoration button.titlebutton {
                                min-height: 18px;
                                min-width: 18px;
                                padding: 2px;
                                margin-top: 0px;
                                margin-bottom: 0px;
                            }
                        ";
                        gtk_css_provider_load_from_data(
                            provider,
                            css.as_ptr(),
                            -1,
                            std::ptr::null_mut(),
                        );
                        gtk_style_context_add_provider_for_screen(
                            screen,
                            provider,
                            GTK_STYLE_PROVIDER_PRIORITY_USER,
                        );
                    }
                }
            });

            let list = gtk_window_list_toplevels();
            let mut node = list;
            while !node.is_null() {
                let win = (*node).data;
                if !win.is_null() {
                    let tb = gtk_window_get_titlebar(win);
                    if !tb.is_null() {
                        let tb_sc = gtk_widget_get_style_context(tb);
                        if !tb_sc.is_null() {
                            gtk_style_context_add_class(tb_sc, c"default-decoration".as_ptr());
                            gtk_style_context_add_class(tb_sc, c"titlebar".as_ptr());
                        }
                        let child = gtk_bin_get_child(tb);
                        if !child.is_null() {
                            gtk_header_bar_set_has_subtitle(child, 0);
                            let hb_sc = gtk_widget_get_style_context(child);
                            if !hb_sc.is_null() {
                                gtk_style_context_add_class(hb_sc, c"default-decoration".as_ptr());
                                gtk_style_context_add_class(hb_sc, c"titlebar".as_ptr());
                            }
                        }
                        gtk_widget_queue_resize(win);
                    }
                }
                node = (*node).next;
            }
            if !list.is_null() {
                g_list_free(list);
            }
        }
    }
}

fn main() -> wry::Result<()> {
    let paths = resolve_paths();
    let _ = std::fs::create_dir_all(&paths.config_dir);
    let _ = std::fs::create_dir_all(&paths.data_dir);

    let controller_instance = ClientController::with_paths(&paths.config_dir, &paths.data_dir);
    #[cfg(target_os = "linux")]
    linux_runtime::configure_pre_init(&controller_instance);

    let controller = Arc::new(Mutex::new(controller_instance));

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

    let mut window_builder = WindowBuilder::new()
        .with_title("Blockidle")
        .with_inner_size(LogicalSize::new(width, height))
        .with_fullscreen(if fullscreen {
            Some(Fullscreen::Borderless(None))
        } else {
            None
        });
    if let Some(icon) = load_app_window_icon() {
        window_builder = window_builder.with_window_icon(Some(icon));
    }
    let window = window_builder
        .build(&event_loop)
        .expect("failed to build Client Shell window");

    #[cfg(target_os = "linux")]
    linux_runtime::apply_gtk_settings();

    let tray = build_system_tray(initial_tray_state, proxy.clone());

    let mut web_context = WebContext::new(Some(data_dir));
    let webview = build_game_view(&window, &mut web_context, &controller, proxy)?;

    #[cfg(target_os = "linux")]
    linux_runtime::apply_gtk_settings();

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
