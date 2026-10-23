use blockidle_app::{
    BackgroundFpsCap, ClientController, ClientSettings, ForegroundFpsCap, Keybindings,
    NavigationDecision, PlatformEffect, TrayAction, TrayMenuState,
};
use tempfile::tempdir;

const MOCK_DOM_PREAMBLE: &str = r#"
let now = 1000;
let nativeRafCallbacks = [];
let nextNativeRafId = 1;
let timers = [];
let nextTimerId = 1;
let nativeHidden = false;
let nativeVisibilityState = 'visible';
const ipcMessages = [];
const winListeners = [];
const docListeners = [];

function createMockNode(tagName) {
    const children = [];
    const elementsById = new Map();
    const listeners = {};
    const attributes = {};
    const node = {
        tagName: (tagName || 'DIV').toUpperCase(),
        id: '',
        className: '',
        type: '',
        value: '',
        checked: false,
        disabled: false,
        style: {},
        dataset: {},
        textContent: '',
        innerHTML: '',
        children: children,
        shadowRoot: null,
        attachShadow(opts) {
            const root = createMockNode('SHADOW-ROOT');
            root.mode = opts && opts.mode;
            this.shadowRoot = root;
            return root;
        },
        appendChild(child) {
            children.push(child);
            if (child && child.id) {
                elementsById.set(child.id, child);
            }
            return child;
        },
        getElementById(id) {
            if (elementsById.has(id)) return elementsById.get(id);
            for (const c of children) {
                if (c.id === id) return c;
                if (c.getElementById) {
                    const found = c.getElementById(id);
                    if (found) return found;
                }
            }
            return null;
        },
        querySelector(sel) {
            if (sel.startsWith('#')) {
                return this.getElementById(sel.slice(1));
            }
            return null;
        },
        querySelectorAll() {
            return [];
        },
        setAttribute(k, v) {
            attributes[k] = String(v);
            if (k === 'id') this.id = String(v);
        },
        getAttribute(k) {
            return attributes[k] || null;
        },
        addEventListener(type, fn) {
            if (!listeners[type]) listeners[type] = [];
            listeners[type].push(fn);
        },
        dispatchEvent(ev) {
            const list = listeners[ev.type] || [];
            for (const fn of list) {
                fn.call(this, ev);
            }
        },
        click() {
            this.dispatchEvent({ type: 'click', target: this, currentTarget: this, preventDefault() {}, stopPropagation() {} });
        },
        change(newValue, newChecked) {
            if (newValue !== undefined) this.value = newValue;
            if (newChecked !== undefined) this.checked = newChecked;
            this.dispatchEvent({ type: 'change', target: this, currentTarget: this, preventDefault() {}, stopPropagation() {} });
        }
    };
    return node;
}

const body = createMockNode('BODY');
const documentElement = createMockNode('HTML');
const document = {
    readyState: 'complete',
    get hidden() { return nativeHidden; },
    get visibilityState() { return nativeVisibilityState; },
    body: body,
    documentElement: documentElement,
    addEventListener(type, listener, options) {
        const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
        docListeners.push({ type, listener, capture });
    },
    dispatchVisibilityChange() {
        let stopped = false;
        const event = {
            type: 'visibilitychange',
            stopImmediatePropagation() { stopped = true; },
            stopPropagation() { stopped = true; },
            preventDefault() {}
        };
        const sorted = docListeners
            .filter(l => l.type === 'visibilitychange')
            .sort((a, b) => (b.capture ? 1 : 0) - (a.capture ? 1 : 0));
        for (const item of sorted) {
            if (stopped) break;
            item.listener(event);
        }
    },
    createElement: (tag) => createMockNode(tag),
    getElementById: (id) => body.getElementById(id) || documentElement.getElementById(id),
    querySelectorAll: () => []
};

const window = {
    performance: { now: () => now },
    requestAnimationFrame: (cb) => {
        const id = nextNativeRafId++;
        nativeRafCallbacks.push({ id, cb });
        return id;
    },
    cancelAnimationFrame: (id) => {
        nativeRafCallbacks = nativeRafCallbacks.filter(item => item.id !== id);
    },
    setTimeout: (cb, delay) => {
        const id = nextTimerId++;
        timers.push({ id, cb, runAt: now + Math.max(0, delay || 0) });
        return id;
    },
    clearTimeout: (id) => {
        timers = timers.filter(item => item.id !== id);
    },
    setInterval: () => 1,
    clearInterval: () => {},
    addEventListener(type, listener, options) {
        const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
        winListeners.push({ type, listener, capture });
    },
    dispatchEvent: () => true,
    CustomEvent: class CustomEvent { constructor(type, opts) { this.type = type; this.detail = opts ? opts.detail : undefined; } },
    ipc: {
        postMessage(raw) {
            ipcMessages.push(JSON.parse(raw));
        }
    },
    document: document
};
globalThis.window = window;
globalThis.document = document;
globalThis.performance = window.performance;
globalThis.requestAnimationFrame = (...args) => window.requestAnimationFrame(...args);
globalThis.cancelAnimationFrame = (...args) => window.cancelAnimationFrame(...args);
globalThis.setTimeout = window.setTimeout;
globalThis.clearTimeout = window.clearTimeout;
globalThis.CustomEvent = window.CustomEvent;

function stepNativeFrame(dtMs) {
    now += dtMs;
    const dueTimers = timers.filter(t => t.runAt <= now);
    timers = timers.filter(t => t.runAt > now);
    for (const t of dueTimers) {
        t.cb();
    }
    const batch = nativeRafCallbacks.slice();
    nativeRafCallbacks = [];
    for (const item of batch) {
        item.cb(now);
    }
}

function dispatchKey(type, key, opts = {}) {
    let immediateStopped = false;
    let defaultPrevented = false;
    const event = {
        type,
        key,
        code: opts.code || key,
        ctrlKey: !!opts.ctrlKey,
        altKey: !!opts.altKey,
        shiftKey: !!opts.shiftKey,
        metaKey: !!opts.metaKey,
        repeat: !!opts.repeat,
        preventDefault() { defaultPrevented = true; },
        stopImmediatePropagation() { immediateStopped = true; },
        stopPropagation() { immediateStopped = true; }
    };
    const all = [
        ...winListeners.filter(l => l.type === type && l.capture),
        ...docListeners.filter(l => l.type === type && l.capture),
        ...winListeners.filter(l => l.type === type && !l.capture),
        ...docListeners.filter(l => l.type === type && !l.capture),
    ];
    for (const item of all) {
        if (immediateStopped) break;
        item.listener(event);
    }
    return { immediateStopped, defaultPrevented };
}
"#;

fn run_node_with_dom(test_name: &str, init_script: &str, test_body: &str) -> String {
    let full_script = format!("{MOCK_DOM_PREAMBLE}\n{init_script}\n{test_body}");
    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(&full_script)
        .output()
        .expect("run node test");
    assert!(
        output.status.success(),
        "node {test_name} failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn initializes_default_client_settings_and_game_view_url_when_config_missing() {
    let dir = tempdir().expect("tempdir");
    let controller = ClientController::load_or_default(dir.path());

    assert_eq!(controller.game_view_url(), "https://blockidle.io/play");

    let settings = controller.settings();
    assert_eq!(settings.foreground_fps_cap, ForegroundFpsCap::Unlimited);
    assert_eq!(settings.background_fps_cap, BackgroundFpsCap::Fps15);
    assert!(settings.keep_awake);
    assert!(!settings.close_to_tray);
    assert!(!settings.audio_muted);
    assert!(!settings.fullscreen);
    assert!(!settings.fps_hud_visible);
    assert_eq!(settings.window_width, 1280);
    assert_eq!(settings.window_height, 720);
    assert_eq!(
        settings.keybindings,
        Keybindings {
            toggle_settings_overlay: "F2".to_string(),
            toggle_fps_hud: "F3".to_string(),
            reload_game_view: "F5".to_string(),
            hide_to_tray: "F9".to_string(),
            toggle_audio_mute: "F10".to_string(),
            toggle_fullscreen: "F11".to_string(),
        }
    );
}

#[test]
fn persists_client_settings_to_disk_and_restores_on_reload() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    let updated = ClientSettings {
        foreground_fps_cap: ForegroundFpsCap::Fps144,
        background_fps_cap: BackgroundFpsCap::Fps5,
        fps_hud_visible: true,
        keep_awake: false,
        close_to_tray: true,
        audio_muted: true,
        fullscreen: true,
        window_width: 1920,
        window_height: 1080,
        keybindings: Keybindings {
            toggle_settings_overlay: "Ctrl+F2".to_string(),
            toggle_fps_hud: "F4".to_string(),
            reload_game_view: "Ctrl+R".to_string(),
            hide_to_tray: "Alt+F9".to_string(),
            toggle_audio_mute: "F8".to_string(),
            toggle_fullscreen: "Alt+Enter".to_string(),
        },
    };

    controller
        .update_settings(updated.clone())
        .expect("save settings");

    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings(), &updated);
}

#[test]
fn falls_back_to_defaults_when_settings_file_is_corrupted_and_provides_persistent_data_dir() {
    let config_dir = tempdir().expect("config tempdir");
    let data_dir = tempdir().expect("data tempdir");
    std::fs::write(config_dir.path().join("settings.json"), "{ corrupted json !!!")
        .expect("write corrupted file");

    let controller = ClientController::with_paths(config_dir.path(), data_dir.path());
    assert_eq!(controller.settings(), &ClientSettings::default());
    assert_eq!(controller.data_dir(), data_dir.path());

    let os_paths = blockidle_app::AppPaths::resolve().expect("resolve standard OS paths");
    assert!(
        os_paths.config_dir.ends_with("blockidle")
            || os_paths.config_dir.ends_with("Blockidle")
            || !os_paths.config_dir.as_os_str().is_empty()
    );
    assert!(!os_paths.data_dir.as_os_str().is_empty());
}

#[test]
fn falls_back_to_defaults_and_persists_clean_settings_when_settings_json_has_invalid_or_duplicate_keybindings() {
    let dir = tempdir().expect("tempdir");
    let settings_path = dir.path().join("settings.json");

    // 1. Invalid keybinding ("Escape" is reserved for cancelling/closing overlay)
    std::fs::write(
        &settings_path,
        r#"{
            "foreground_fps_cap": "Fps60",
            "keybindings": {
                "toggle_settings_overlay": "Escape",
                "toggle_fps_hud": "F3",
                "reload_game_view": "F5",
                "hide_to_tray": "F9",
                "toggle_audio_mute": "F10",
                "toggle_fullscreen": "F11"
            }
        }"#,
    )
    .expect("write invalid keybinding settings");

    let controller = ClientController::load_or_default(dir.path());
    assert_eq!(controller.settings(), &ClientSettings::default());
    let persisted_after_invalid: ClientSettings =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).expect("read persisted"))
            .expect("parse persisted clean settings");
    assert_eq!(persisted_after_invalid, ClientSettings::default());

    // 2. Duplicate keybinding (two actions bound to "F2")
    std::fs::write(
        &settings_path,
        r#"{
            "foreground_fps_cap": "Fps120",
            "keybindings": {
                "toggle_settings_overlay": "F2",
                "toggle_fps_hud": "f2",
                "reload_game_view": "F5",
                "hide_to_tray": "F9",
                "toggle_audio_mute": "F10",
                "toggle_fullscreen": "F11"
            }
        }"#,
    )
    .expect("write duplicate keybinding settings");

    let controller_dup = ClientController::load_or_default(dir.path());
    assert_eq!(controller_dup.settings(), &ClientSettings::default());
    let persisted_after_dup: ClientSettings =
        serde_json::from_str(&std::fs::read_to_string(&settings_path).expect("read persisted"))
            .expect("parse persisted clean settings");
    assert_eq!(persisted_after_dup, ClientSettings::default());
}

#[test]
fn classifies_navigation_policy_for_game_view_stripe_turnstile_and_external_links() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    for allowed_url in [
        "https://blockidle.io/play",
        "https://blockidle.io/play?pago=pi_12345",
        "https://sub.blockidle.io/auth",
        "https://challenges.cloudflare.com/turnstile/v0/api.js",
        "https://checkout.stripe.com/c/pay/cs_live_123",
        "https://stripe.com/docs",
    ] {
        let (decision, effects) = controller.handle_navigation_request(allowed_url);
        assert_eq!(
            decision,
            NavigationDecision::AllowInGameView,
            "expected {allowed_url} to stay inside Game View"
        );
        assert!(effects.is_empty());
    }

    let discord_url = "https://discord.gg/blockidle";
    let (decision, effects) = controller.handle_navigation_request(discord_url);
    assert_eq!(
        decision,
        NavigationDecision::OpenExternalBrowser(discord_url.to_string())
    );
    assert_eq!(
        effects,
        vec![PlatformEffect::OpenExternalBrowser(discord_url.to_string())]
    );

    let (popup_decision, popup_effects) = controller.handle_new_window_request(discord_url);
    assert_eq!(
        popup_decision,
        NavigationDecision::OpenExternalBrowser(discord_url.to_string())
    );
    assert_eq!(
        popup_effects,
        vec![PlatformEffect::OpenExternalBrowser(discord_url.to_string())]
    );
}

#[test]
fn generates_initialization_script_and_processes_ipc_messages_from_game_view() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    let init_script = controller.initialization_script();
    assert!(init_script.contains("window.ipc.postMessage"));
    assert!(init_script.contains("__BLOCKIDLE_CLIENT_STATE__"));
    assert!(init_script.contains("__blockidleApplyState"));
    assert!(init_script.contains("\"keep_awake\":true"));

    // Simulate Game View sending `client_ready` over `window.ipc.postMessage`
    let ready_effects = controller.handle_ipc_message(r#"{"type":"client_ready"}"#);
    assert_eq!(ready_effects.len(), 1);
    match &ready_effects[0] {
        PlatformEffect::SyncWebViewState(script) => {
            assert!(script.contains("__blockidleApplyState"));
            assert!(script.contains("\"keep_awake\":true"));
        }
        other => panic!("expected SyncWebViewState, got {other:?}"),
    }

    // Simulate Game View updating settings via IPC and verify persistence + host effects
    let mut new_settings = controller.settings().clone();
    new_settings.foreground_fps_cap = ForegroundFpsCap::Fps60;
    new_settings.fullscreen = true;
    let ipc_payload = serde_json::json!({
        "type": "update_settings",
        "settings": new_settings,
    })
    .to_string();

    let effects = controller.handle_ipc_message(&ipc_payload);
    assert_eq!(controller.settings().foreground_fps_cap, ForegroundFpsCap::Fps60);
    assert!(controller.settings().fullscreen);
    assert!(effects.contains(&PlatformEffect::SetFullscreen(true)));
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(_)))
    );

    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings().foreground_fps_cap, ForegroundFpsCap::Fps60);
    assert!(reloaded.settings().fullscreen);
}

#[test]
fn persists_window_dimensions_on_resize_and_handles_window_lifecycle_events() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    controller.handle_window_resized(1600, 900);
    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(reloaded.settings().window_width, 1600);
    assert_eq!(reloaded.settings().window_height, 900);

    let close_effects = controller.handle_close_requested();
    assert_eq!(close_effects, vec![PlatformEffect::ExitApp]);
}

#[test]
fn tray_actions_hide_and_restore_window_switching_between_background_and_foreground_fps_caps_and_quit() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    let mut settings = controller.settings().clone();
    settings.foreground_fps_cap = ForegroundFpsCap::Fps144;
    settings.background_fps_cap = BackgroundFpsCap::Fps5;
    controller.update_settings(settings).expect("update settings");

    assert!(controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(144));

    let hide_effects = controller.handle_tray_action(TrayAction::Hide);
    assert!(!controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(5));
    assert!(hide_effects.contains(&PlatformEffect::SetWindowVisible(false)));
    assert!(hide_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: false,
        audio_muted: false,
    })));
    assert!(
        hide_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"effective_fps_cap\":5")))
    );

    let show_effects = controller.handle_tray_action(TrayAction::Show);
    assert!(controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(144));
    assert!(show_effects.contains(&PlatformEffect::SetWindowVisible(true)));
    assert!(show_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: false,
    })));
    assert!(
        show_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"effective_fps_cap\":144")))
    );

    // Minimizing prior to hiding to tray and then restoring from tray clears minimized state and restores Foreground FPS Cap
    controller.handle_minimized_changed(true);
    assert_eq!(controller.effective_fps_cap(), Some(5));
    controller.handle_tray_action(TrayAction::Hide);
    assert_eq!(controller.effective_fps_cap(), Some(5));
    let restore_after_min_effects = controller.handle_tray_action(TrayAction::Show);
    assert!(controller.is_window_visible());
    assert!(!controller.webview_state().window_minimized);
    assert_eq!(controller.effective_fps_cap(), Some(144));
    assert!(
        restore_after_min_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"effective_fps_cap\":144") && script.contains("\"window_minimized\":false")))
    );

    let toggle_hide_effects = controller.handle_tray_action(TrayAction::ToggleWindowVisibility);
    assert!(!controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(5));
    assert!(toggle_hide_effects.contains(&PlatformEffect::SetWindowVisible(false)));

    let toggle_show_effects = controller.handle_tray_action(TrayAction::ToggleWindowVisibility);
    assert!(controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(144));
    assert!(toggle_show_effects.contains(&PlatformEffect::SetWindowVisible(true)));

    let quit_effects = controller.handle_tray_action(TrayAction::Quit);
    assert_eq!(quit_effects, vec![PlatformEffect::ExitApp]);
}

#[test]
fn close_requested_exits_when_close_to_tray_disabled_and_hides_to_tray_when_enabled() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    assert!(!controller.settings().close_to_tray);
    assert_eq!(
        controller.handle_close_requested(),
        vec![PlatformEffect::ExitApp]
    );
    assert!(controller.is_window_visible());

    let mut settings = controller.settings().clone();
    settings.close_to_tray = true;
    settings.foreground_fps_cap = ForegroundFpsCap::Fps60;
    settings.background_fps_cap = BackgroundFpsCap::Fps0;
    controller.update_settings(settings).expect("save settings");

    assert_eq!(controller.effective_fps_cap(), Some(60));

    let close_effects = controller.handle_close_requested();
    assert!(!controller.is_window_visible());
    assert_eq!(controller.effective_fps_cap(), Some(0));
    assert!(!close_effects.contains(&PlatformEffect::ExitApp));
    assert!(close_effects.contains(&PlatformEffect::SetWindowVisible(false)));
    assert!(close_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: false,
        audio_muted: false,
    })));
    assert!(
        close_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"effective_fps_cap\":0") && script.contains("\"window_visible\":false")))
    );
}

#[test]
fn switches_effective_fps_cap_between_foreground_and_background_on_focus_and_minimize_transitions() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    // Default: Unlimited in foreground, Fps15 in background
    assert_eq!(controller.effective_fps_cap(), None);
    assert_eq!(controller.webview_state().effective_fps_cap, None);

    // Unfocus switches to background cap (15 FPS) and emits SyncWebViewState
    let unfocus_effects = controller.handle_focus_changed(false);
    assert_eq!(controller.effective_fps_cap(), Some(15));
    assert_eq!(controller.webview_state().effective_fps_cap, Some(15));
    assert_eq!(
        unfocus_effects,
        vec![PlatformEffect::SyncWebViewState(
            controller.webview_state_sync_script()
        )]
    );

    // Focus restores foreground cap (Unlimited -> None)
    let focus_effects = controller.handle_focus_changed(true);
    assert_eq!(controller.effective_fps_cap(), None);
    assert_eq!(
        focus_effects,
        vec![PlatformEffect::SyncWebViewState(
            controller.webview_state_sync_script()
        )]
    );

    // Test all foreground presets when focused & all background presets when minimized
    for (fg_preset, expected_fg) in [
        (ForegroundFpsCap::Fps15, Some(15)),
        (ForegroundFpsCap::Fps30, Some(30)),
        (ForegroundFpsCap::Fps60, Some(60)),
        (ForegroundFpsCap::Fps120, Some(120)),
        (ForegroundFpsCap::Fps144, Some(144)),
        (ForegroundFpsCap::Fps240, Some(240)),
        (ForegroundFpsCap::Unlimited, None),
    ] {
        for (bg_preset, expected_bg) in [
            (BackgroundFpsCap::Fps0, Some(0)),
            (BackgroundFpsCap::Fps5, Some(5)),
            (BackgroundFpsCap::Fps15, Some(15)),
            (BackgroundFpsCap::Fps30, Some(30)),
            (BackgroundFpsCap::Fps60, Some(60)),
        ] {
            let mut settings = controller.settings().clone();
            settings.foreground_fps_cap = fg_preset;
            settings.background_fps_cap = bg_preset;
            controller.update_settings(settings).expect("update settings");

            // While focused and not minimized -> foreground cap
            controller.handle_minimized_changed(false);
            controller.handle_focus_changed(true);
            assert_eq!(controller.effective_fps_cap(), expected_fg);
            assert_eq!(controller.webview_state().effective_fps_cap, expected_fg);

            // Minimized while focused -> background cap
            let min_effects = controller.handle_minimized_changed(true);
            assert_eq!(controller.effective_fps_cap(), expected_bg);
            assert_eq!(controller.webview_state().effective_fps_cap, expected_bg);
            assert_eq!(
                min_effects,
                vec![PlatformEffect::SyncWebViewState(
                    controller.webview_state_sync_script()
                )]
            );

            // Restore from minimize while focused -> foreground cap
            let restore_effects = controller.handle_minimized_changed(false);
            assert_eq!(controller.effective_fps_cap(), expected_fg);
            assert_eq!(
                restore_effects,
                vec![PlatformEffect::SyncWebViewState(
                    controller.webview_state_sync_script()
                )]
            );
        }
    }

    // Zero-dimension resize (common when minimizing on Windows/Linux) also activates minimized state without overwriting saved dimensions
    let mut settings = controller.settings().clone();
    settings.foreground_fps_cap = ForegroundFpsCap::Fps120;
    settings.background_fps_cap = BackgroundFpsCap::Fps0;
    controller.update_settings(settings).expect("update settings");
    let zero_resize_effects = controller.handle_window_resized(0, 0);
    assert_eq!(controller.effective_fps_cap(), Some(0));
    assert_eq!(controller.settings().window_width, 1280);
    assert_eq!(
        zero_resize_effects,
        vec![PlatformEffect::SyncWebViewState(
            controller.webview_state_sync_script()
        )]
    );

    let restore_resize_effects = controller.handle_window_resized(1600, 900);
    assert_eq!(controller.effective_fps_cap(), Some(120));
    assert_eq!(controller.settings().window_width, 1600);
    assert_eq!(
        restore_resize_effects,
        vec![PlatformEffect::SyncWebViewState(
            controller.webview_state_sync_script()
        )]
    );
}

#[test]
fn initialization_script_enforces_raf_pacing_zero_fps_pause_and_flush_and_cancellation() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());
    let mut settings = controller.settings().clone();
    settings.foreground_fps_cap = ForegroundFpsCap::Fps60;
    settings.background_fps_cap = BackgroundFpsCap::Fps0;
    controller.update_settings(settings).expect("update settings");

    let init_script = controller.initialization_script();
    let unfocus_sync_script = controller
        .handle_focus_changed(false)
        .into_iter()
        .find_map(|e| match e {
            PlatformEffect::SyncWebViewState(s) => Some(s),
            _ => None,
        })
        .expect("unfocus sync script");
    let focus_sync_script = controller
        .handle_focus_changed(true)
        .into_iter()
        .find_map(|e| match e {
            PlatformEffect::SyncWebViewState(s) => Some(s),
            _ => None,
        })
        .expect("focus sync script");

    assert!(init_script.contains("requestAnimationFrame"));
    assert!(init_script.contains("cancelAnimationFrame"));

    let test_body = format!(
        r#"
// 1. At 60 FPS cap (interval ~16.666ms), a recursive game loop running on a 240Hz display (4ms native ticks)
// should only fire ~60 times over 1000ms (not 250 times).
let deliveredFrames = 0;
function gameLoop() {{
    deliveredFrames++;
    window.requestAnimationFrame(gameLoop);
}}
window.requestAnimationFrame(gameLoop);

for (let i = 0; i < 250; i++) {{
    stepNativeFrame(4);
}}
if (deliveredFrames < 55 || deliveredFrames > 65) {{
    throw new Error('Expected ~60 frames at 60 FPS cap over 1000ms, got ' + deliveredFrames);
}}

// 2. CancelAnimationFrame prevents a scheduled callback from firing
let cancelledFired = false;
const cancelId = window.requestAnimationFrame(() => {{ cancelledFired = true; }});
window.cancelAnimationFrame(cancelId);
for (let i = 0; i < 10; i++) {{
    stepNativeFrame(20);
}}
if (cancelledFired) {{
    throw new Error('Cancelled requestAnimationFrame callback should not have fired');
}}

// 2b. CancelAnimationFrame called inside callback A cancels callback B in the same frame batch
let callbackAFired = false;
let callbackBFired = false;
let idB = null;
window.requestAnimationFrame(() => {{
    callbackAFired = true;
    window.cancelAnimationFrame(idB);
}});
idB = window.requestAnimationFrame(() => {{
    callbackBFired = true;
}});
for (let i = 0; i < 10; i++) {{
    stepNativeFrame(20);
}}
if (!callbackAFired) {{
    throw new Error('Expected callback A to fire');
}}
if (callbackBFired) {{
    throw new Error('Expected callback B cancelled by callback A in the same batch not to fire');
}}

// 3. Switching to 0 FPS background cap pauses callbacks completely
const beforePause = deliveredFrames;
{unfocus_sync_script}
for (let i = 0; i < 50; i++) {{
    stepNativeFrame(20);
}}
if (deliveredFrames !== beforePause) {{
    throw new Error('Expected 0 frames delivered while cap is 0 FPS, but got ' + (deliveredFrames - beforePause));
}}

// 4. Returning to foreground (> 0 FPS cap) flushes queued callback on the next frame
{focus_sync_script}
stepNativeFrame(4);
if (deliveredFrames <= beforePause) {{
    throw new Error('Expected pending rAF callbacks to flush immediately when returning from 0 FPS cap');
}}
console.log('RAF_TEST_OK');
"#
    );

    run_node_with_dom("raf", &init_script, &test_body);
}

#[test]
fn toggles_audio_mute_from_tray_and_ipc_persists_state_syncs_tray_menu_and_injects_audio_interceptor() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    assert!(!controller.settings().audio_muted);
    let init_script = controller.initialization_script();
    assert!(init_script.contains("AudioContext"));
    assert!(init_script.contains("webkitAudioContext"));
    assert!(init_script.contains("HTMLMediaElement"));

    // Toggle mute on via system tray action
    let mute_effects = controller.handle_tray_action(TrayAction::ToggleMute);
    assert!(controller.settings().audio_muted);
    assert!(mute_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: true,
    })));
    assert!(
        mute_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"audio_muted\":true")))
    );

    // Verify persistence on disk
    let reloaded = ClientController::load_or_default(dir.path());
    assert!(reloaded.settings().audio_muted);

    // Toggle mute off via IPC message
    let unmute_effects = controller.handle_ipc_message(r#"{"type":"toggle_audio_mute"}"#);
    assert!(!controller.settings().audio_muted);
    assert!(unmute_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: false,
    })));
    assert!(
        unmute_effects
            .iter()
            .any(|effect| matches!(effect, PlatformEffect::SyncWebViewState(script) if script.contains("\"audio_muted\":false")))
    );

    // Updating settings via IPC also synchronizes tray menu when audio_muted changes
    let mut updated_settings = controller.settings().clone();
    updated_settings.audio_muted = true;
    let ipc_payload = serde_json::json!({
        "type": "update_settings",
        "settings": updated_settings,
    })
    .to_string();
    let settings_effects = controller.handle_ipc_message(&ipc_payload);
    assert!(controller.settings().audio_muted);
    assert!(settings_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: true,
    })));
}

#[test]
fn initialization_script_enforces_keep_awake_visibility_suppression_when_enabled() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());
    assert!(controller.settings().keep_awake);

    let init_script = controller.initialization_script();

    let mut disabled_settings = controller.settings().clone();
    disabled_settings.keep_awake = false;
    controller
        .update_settings(disabled_settings)
        .expect("update settings");
    let disable_keep_awake_script = controller.webview_state_sync_script();

    assert!(init_script.contains("visibilitychange"));
    assert!(init_script.contains("visibilityState"));

    let test_body = format!(
        r#"
let gameVisibilityCalls = 0;
document.addEventListener('visibilitychange', () => {{
    gameVisibilityCalls++;
}});

// Simulate browser/WebView hiding the page while Keep Awake is true
nativeHidden = true;
nativeVisibilityState = 'hidden';
document.dispatchVisibilityChange();

if (document.hidden !== false) {{
    throw new Error('Expected document.hidden === false when Keep Awake is enabled, got ' + document.hidden);
}}
if (document.visibilityState !== 'visible') {{
    throw new Error('Expected document.visibilityState === "visible" when Keep Awake is enabled, got ' + document.visibilityState);
}}
if (gameVisibilityCalls !== 0) {{
    throw new Error('Expected visibilitychange listener to be suppressed when Keep Awake is enabled, got ' + gameVisibilityCalls);
}}

// Disable Keep Awake via state sync and verify native visibility signals work again
{disable_keep_awake_script}
document.dispatchVisibilityChange();
if (document.hidden !== true) {{
    throw new Error('Expected document.hidden === true when Keep Awake is disabled');
}}
if (document.visibilityState !== 'hidden') {{
    throw new Error('Expected document.visibilityState === "hidden" when Keep Awake is disabled');
}}
if (gameVisibilityCalls !== 1) {{
    throw new Error('Expected visibilitychange listener to fire when Keep Awake is disabled');
}}
console.log('KEEP_AWAKE_OK');
"#
    );

    run_node_with_dom("keep-awake", &init_script, &test_body);
}

#[test]
fn initialization_script_renders_shadow_dom_fps_hud_with_pointer_events_none_and_respects_visibility_setting() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());
    assert!(!controller.settings().fps_hud_visible);

    let init_script = controller.initialization_script();

    // Enable FPS HUD and set Foreground FPS Cap to 144
    let mut hud_on = controller.settings().clone();
    hud_on.fps_hud_visible = true;
    hud_on.foreground_fps_cap = ForegroundFpsCap::Fps144;
    hud_on.background_fps_cap = BackgroundFpsCap::Fps0;
    controller.update_settings(hud_on).expect("update settings");
    let show_hud_script = controller.webview_state_sync_script();

    // Unfocus -> Background FPS Cap (0 FPS)
    let unfocus_script = controller
        .handle_focus_changed(false)
        .into_iter()
        .find_map(|e| match e {
            PlatformEffect::SyncWebViewState(s) => Some(s),
            _ => None,
        })
        .expect("unfocus sync script");

    // Focus with Unlimited cap
    let _ = controller.handle_focus_changed(true);
    let mut unlimited_settings = controller.settings().clone();
    unlimited_settings.foreground_fps_cap = ForegroundFpsCap::Unlimited;
    controller
        .update_settings(unlimited_settings)
        .expect("update settings");
    let unlimited_script = controller.webview_state_sync_script();

    assert!(init_script.contains("attachShadow"));
    assert!(init_script.contains("pointer-events: none"));

    let test_body = format!(
        r#"
const host = document.getElementById('blockidle-client-overlay-host');
if (!host || !host.shadowRoot) {{
    throw new Error('Expected Shadow DOM host #blockidle-client-overlay-host to be mounted');
}}

const hud = host.shadowRoot.getElementById('blockidle-fps-hud');
if (!hud) {{
    throw new Error('Expected #blockidle-fps-hud inside Shadow DOM');
}}

// Initially fps_hud_visible is false -> hidden
if (hud.style.display !== 'none') {{
    throw new Error('Expected FPS HUD to be hidden initially when fps_hud_visible is false, got display=' + hud.style.display);
}}

// Enable FPS HUD with 144 FPS cap
{show_hud_script}
if (hud.style.display === 'none') {{
    throw new Error('Expected FPS HUD to be visible when fps_hud_visible is true');
}}
if (!hud.textContent.includes('144')) {{
    throw new Error('Expected FPS HUD text to display active cap 144, got: ' + hud.textContent);
}}

// Unfocus -> 0 FPS cap (Paused)
{unfocus_script}
if (!hud.textContent.includes('0') && !hud.textContent.toLowerCase().includes('paused')) {{
    throw new Error('Expected FPS HUD text to reflect 0 FPS background cap, got: ' + hud.textContent);
}}

// Focus with Unlimited cap and deliver 30 frames within 1s window
{unlimited_script}
for (let i = 0; i < 30; i++) {{
    window.requestAnimationFrame(() => {{}});
    stepNativeFrame(10);
}}
if (!hud.textContent.includes('30') || !hud.textContent.includes('Unlimited')) {{
    throw new Error('Expected FPS HUD to show 30 FPS and Unlimited cap, got: ' + hud.textContent);
}}
console.log('HUD_TEST_OK');
"#
    );

    run_node_with_dom("fps-hud", &init_script, &test_body);
}

#[test]
fn validates_normalizes_prevents_conflicts_and_restores_default_keybindings() {
    use blockidle_app::{KeybindingAction, KeybindingError, normalize_keybinding};

    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    // Normalization converts case and orders modifiers canonically (Ctrl+Alt+Shift+Key)
    assert_eq!(normalize_keybinding("f2"), Some("F2".to_string()));
    assert_eq!(
        normalize_keybinding("shift+ctrl+s"),
        Some("Ctrl+Shift+S".to_string())
    );
    assert_eq!(
        normalize_keybinding("alt + ctrl + enter"),
        Some("Ctrl+Alt+Enter".to_string())
    );

    // Invalid shortcuts (empty, modifier-only, Escape) are rejected
    assert_eq!(normalize_keybinding(""), None);
    assert_eq!(normalize_keybinding("Ctrl+Shift"), None);
    assert_eq!(normalize_keybinding("Escape"), None);
    assert_eq!(normalize_keybinding("Esc"), None);

    assert_eq!(
        controller.update_keybinding(KeybindingAction::ToggleFpsHud, "Ctrl+Shift"),
        Err(KeybindingError::InvalidShortcut("Ctrl+Shift".to_string()))
    );
    assert_eq!(
        controller.update_keybinding(KeybindingAction::ToggleFpsHud, "Escape"),
        Err(KeybindingError::InvalidShortcut("Escape".to_string()))
    );

    // Duplicate binding conflict prevention (F2 is already ToggleSettingsOverlay)
    assert_eq!(
        controller.update_keybinding(KeybindingAction::ToggleFpsHud, "f2"),
        Err(KeybindingError::DuplicateShortcut {
            shortcut: "F2".to_string(),
            existing_action: KeybindingAction::ToggleSettingsOverlay,
        })
    );
    assert_eq!(controller.settings().keybindings.toggle_fps_hud, "F3");

    // Valid custom keybinding update normalizes, persists to disk, and syncs WebView state
    let effects = controller
        .update_keybinding(KeybindingAction::ToggleFpsHud, "shift+ctrl+h")
        .expect("valid keybinding update");
    assert_eq!(
        controller.settings().keybindings.toggle_fps_hud,
        "Ctrl+Shift+H"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("Ctrl+Shift+H")))
    );

    // Updating via IPC also normalizes and prevents conflicts
    let ipc_conflict = controller.handle_ipc_message(
        r#"{"type":"update_keybinding","action":"reload_game_view","shortcut":"ctrl+shift+h"}"#,
    );
    assert_eq!(controller.settings().keybindings.reload_game_view, "F5");
    assert!(
        ipc_conflict
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(_)))
    );

    let ipc_ok = controller.handle_ipc_message(
        r#"{"type":"update_keybinding","action":"reload_game_view","shortcut":"alt+r"}"#,
    );
    assert_eq!(controller.settings().keybindings.reload_game_view, "Alt+R");
    assert!(
        ipc_ok
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("Alt+R")))
    );

    let reloaded = ClientController::load_or_default(dir.path());
    assert_eq!(
        reloaded.settings().keybindings.toggle_fps_hud,
        "Ctrl+Shift+H"
    );
    assert_eq!(reloaded.settings().keybindings.reload_game_view, "Alt+R");

    // Restore defaults via IPC resets all shortcuts to F2/F3/F5/F9/F10/F11 and persists to disk
    let restore_effects =
        controller.handle_ipc_message(r#"{"type":"restore_default_keybindings"}"#);
    assert_eq!(controller.settings().keybindings, Keybindings::default());
    assert!(
        restore_effects
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("\"toggle_fps_hud\":\"F3\"")))
    );

    let reloaded_after_restore = ClientController::load_or_default(dir.path());
    assert_eq!(
        reloaded_after_restore.settings().keybindings,
        Keybindings::default()
    );
}

#[test]
fn dispatches_default_and_custom_keybindings_when_focused_and_closes_overlay_on_escape() {
    use blockidle_app::KeybindingAction;

    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());

    assert!(!controller.is_settings_overlay_open());

    // F2 toggles Settings Overlay open
    let f2_open = controller.trigger_keybinding("F2");
    assert!(controller.is_settings_overlay_open());
    assert!(controller.webview_state().settings_overlay_open);
    assert!(
        f2_open
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("\"settings_overlay_open\":true")))
    );

    // Escape closes Settings Overlay when open
    let esc_close = controller.trigger_keybinding("Escape");
    assert!(!controller.is_settings_overlay_open());
    assert!(
        esc_close
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("\"settings_overlay_open\":false")))
    );

    // Escape when Settings Overlay is already closed does nothing
    assert!(controller.trigger_keybinding("Escape").is_empty());

    // F3 toggles FPS HUD and persists
    assert!(!controller.settings().fps_hud_visible);
    let f3_effects = controller.trigger_keybinding("F3");
    assert!(controller.settings().fps_hud_visible);
    assert!(
        f3_effects
            .iter()
            .any(|e| matches!(e, PlatformEffect::SyncWebViewState(s) if s.contains("\"fps_hud_visible\":true")))
    );
    assert!(
        ClientController::load_or_default(dir.path())
            .settings()
            .fps_hud_visible
    );

    // F5 reloads Game View
    assert_eq!(
        controller.trigger_keybinding("F5"),
        vec![PlatformEffect::ReloadGameView]
    );

    // F10 toggles Audio Mute and updates tray + webview
    assert!(!controller.settings().audio_muted);
    let f10_effects = controller.trigger_keybinding("F10");
    assert!(controller.settings().audio_muted);
    assert!(f10_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: true,
    })));

    // F11 toggles Fullscreen and persists
    assert!(!controller.settings().fullscreen);
    let f11_effects = controller.trigger_keybinding("F11");
    assert!(controller.settings().fullscreen);
    assert!(f11_effects.contains(&PlatformEffect::SetFullscreen(true)));

    // Custom keybinding replaces default shortcut
    controller
        .update_keybinding(KeybindingAction::ToggleAudioMute, "Ctrl+Shift+M")
        .expect("rebind mute");
    assert!(controller.trigger_keybinding("F10").is_empty());
    let custom_mute_effects =
        controller.handle_ipc_message(r#"{"type":"trigger_keybinding","shortcut":"ctrl+shift+m"}"#);
    assert!(!controller.settings().audio_muted);
    assert!(custom_mute_effects.contains(&PlatformEffect::UpdateTrayMenu(TrayMenuState {
        window_visible: true,
        audio_muted: false,
    })));

    // F9 hides to system tray (which also unfocuses/hides the window)
    let f9_effects = controller.trigger_keybinding("F9");
    assert!(!controller.is_window_visible());
    assert!(f9_effects.contains(&PlatformEffect::SetWindowVisible(false)));

    // While hidden/unfocused, keybindings are ignored
    assert!(controller.trigger_keybinding("F2").is_empty());
    assert!(controller.trigger_keybinding("F5").is_empty());
    assert!(!controller.is_settings_overlay_open());

    // Restore window focus and verify F2 works again
    controller.restore_from_tray();
    let _ = controller.trigger_keybinding("F2");
    assert!(controller.is_settings_overlay_open());
}

#[test]
fn initialization_script_renders_shadow_dom_settings_overlay_records_keybindings_and_consumes_keys_while_open() {
    let dir = tempdir().expect("tempdir");
    let mut controller = ClientController::load_or_default(dir.path());
    let init_script = controller.initialization_script();

    let test_body = r#"
// Register simulated game keydown/keyup listeners in bubble phase (like blockidle.io js/entrada.js)
let gameKeyEvents = [];
window.addEventListener('keydown', (e) => gameKeyEvents.push('down:' + e.key), false);
window.addEventListener('keyup', (e) => gameKeyEvents.push('up:' + e.key), false);

const host = document.getElementById('blockidle-client-overlay-host');
const shadow = host.shadowRoot;
const overlay = shadow.getElementById('blockidle-settings-overlay');
if (!overlay) {
    throw new Error('Expected #blockidle-settings-overlay inside Shadow DOM');
}
if (overlay.style.display !== 'none') {
    throw new Error('Expected Settings Overlay to be hidden initially');
}

// When overlay is closed, pressing game key 'w' (including repeat) reaches the game
dispatchKey('keydown', 'w');
dispatchKey('keydown', 'w', { repeat: true });
dispatchKey('keyup', 'w');
if (gameKeyEvents.length !== 3) {
    throw new Error('Expected game key w (including repeat) to reach game when overlay is closed');
}
gameKeyEvents = [];

// When overlay is closed, a repeat keydown on a bound shortcut (F3) is consumed without triggering the action
const ipcBeforeRepeatF3 = ipcMessages.length;
const repeatF3Closed = dispatchKey('keydown', 'F3', { repeat: true });
dispatchKey('keyup', 'F3');
if (!repeatF3Closed.immediateStopped || !repeatF3Closed.defaultPrevented) {
    throw new Error('Expected repeat F3 keydown to be consumed');
}
if (ipcMessages.length !== ipcBeforeRepeatF3) {
    throw new Error('Expected repeat F3 keydown not to emit trigger_keybinding IPC');
}

// Pressing F2 opens the Settings Overlay and consumes the key event
const f2Res = dispatchKey('keydown', 'F2');
if (!f2Res.immediateStopped || !f2Res.defaultPrevented) {
    throw new Error('Expected F2 keydown to be consumed in capture phase');
}
if (gameKeyEvents.length !== 0) {
    throw new Error('Expected F2 not to leak to game listeners');
}
if (overlay.style.display === 'none') {
    throw new Error('Expected Settings Overlay to open on F2');
}

// Holding F2 or Escape (event.repeat = true) while Settings Overlay is open is consumed without closing/toggling
const ipcBeforeRepeatWhileOpen = ipcMessages.length;
const repeatF2Open = dispatchKey('keydown', 'F2', { repeat: true });
const repeatEscOpen = dispatchKey('keydown', 'Escape', { repeat: true });
dispatchKey('keyup', 'F2');
if (!repeatF2Open.immediateStopped || !repeatF2Open.defaultPrevented || !repeatEscOpen.immediateStopped || !repeatEscOpen.defaultPrevented) {
    throw new Error('Expected repeat F2/Escape while Settings Overlay is open to be consumed');
}
if (overlay.style.display === 'none') {
    throw new Error('Expected repeat F2/Escape not to close Settings Overlay');
}
if (ipcMessages.length !== ipcBeforeRepeatWhileOpen) {
    throw new Error('Expected repeat F2/Escape while open not to emit IPC');
}

// While Settings Overlay is open, pressing game keys ('w', 'e', ' ') is consumed and does not reach the game
dispatchKey('keydown', 'w');
dispatchKey('keyup', 'w');
dispatchKey('keydown', 'e');
dispatchKey('keyup', 'e');
if (gameKeyEvents.length !== 0) {
    throw new Error('Expected game keys to be consumed while Settings Overlay is open, got: ' + JSON.stringify(gameKeyEvents));
}

// Controls for Foreground FPS Cap, Background FPS Cap, and all 5 toggles exist and emit update_settings IPC
const fgSelect = shadow.getElementById('blockidle-fg-fps-select');
const bgSelect = shadow.getElementById('blockidle-bg-fps-select');
const hudToggle = shadow.getElementById('blockidle-toggle-fps-hud');
const keepAwakeToggle = shadow.getElementById('blockidle-toggle-keep-awake');
const closeToTrayToggle = shadow.getElementById('blockidle-toggle-close-to-tray');
const muteToggle = shadow.getElementById('blockidle-toggle-audio-mute');
const fullscreenToggle = shadow.getElementById('blockidle-toggle-fullscreen');

if (!fgSelect || !bgSelect || !hudToggle || !keepAwakeToggle || !closeToTrayToggle || !muteToggle || !fullscreenToggle) {
    throw new Error('Missing one or more required controls in Settings Overlay');
}

fgSelect.change('Fps144');
bgSelect.change('Fps0');
hudToggle.change(undefined, true);
keepAwakeToggle.change(undefined, false);
closeToTrayToggle.change(undefined, true);
muteToggle.change(undefined, true);
fullscreenToggle.change(undefined, true);

const lastSettingsMsg = [...ipcMessages].reverse().find(m => m.type === 'update_settings');
if (!lastSettingsMsg ||
    lastSettingsMsg.settings.foreground_fps_cap !== 'Fps144' ||
    lastSettingsMsg.settings.background_fps_cap !== 'Fps0' ||
    lastSettingsMsg.settings.fps_hud_visible !== true ||
    lastSettingsMsg.settings.keep_awake !== false ||
    lastSettingsMsg.settings.close_to_tray !== true ||
    lastSettingsMsg.settings.audio_muted !== true ||
    lastSettingsMsg.settings.fullscreen !== true) {
    throw new Error('Expected update_settings IPC to reflect all overlay control changes, got: ' + JSON.stringify(lastSettingsMsg));
}

// Interactive Keybinding recording:
// 1. Click Toggle FPS HUD keybinding button -> enters recording mode, Escape cancels recording WITHOUT closing overlay
const hudKeyBtn = shadow.getElementById('blockidle-keybind-toggle_fps_hud');
if (!hudKeyBtn) {
    throw new Error('Missing #blockidle-keybind-toggle_fps_hud button');
}
hudKeyBtn.click();
if (!hudKeyBtn.textContent.toLowerCase().includes('press')) {
    throw new Error('Expected keybinding button to show recording prompt, got: ' + hudKeyBtn.textContent);
}
dispatchKey('keydown', 'Escape');
dispatchKey('keyup', 'Escape');
if (overlay.style.display === 'none') {
    throw new Error('Expected Escape during shortcut recording to cancel recording without closing the Settings Overlay');
}
if (hudKeyBtn.textContent !== 'F3') {
    throw new Error('Expected cancelled recording to restore F3 label, got: ' + hudKeyBtn.textContent);
}
if (gameKeyEvents.length !== 0) {
    throw new Error('Expected Escape during recording not to leak to game');
}

// 2. Record a conflicting shortcut (F2 is already ToggleSettingsOverlay) -> prevented
hudKeyBtn.click();
dispatchKey('keydown', 'F2');
dispatchKey('keyup', 'F2');
if (hudKeyBtn.textContent !== 'F3') {
    throw new Error('Expected conflicting shortcut F2 to be rejected and keep F3, got: ' + hudKeyBtn.textContent);
}

// 3. Record a valid modifier combination Ctrl+Shift+H -> updates button and emits update_keybinding IPC
hudKeyBtn.click();
dispatchKey('keydown', 'Control', { ctrlKey: true });
dispatchKey('keydown', 'Shift', { ctrlKey: true, shiftKey: true });
dispatchKey('keydown', 'h', { ctrlKey: true, shiftKey: true });
dispatchKey('keyup', 'h', { ctrlKey: true, shiftKey: true });
if (hudKeyBtn.textContent !== 'Ctrl+Shift+H') {
    throw new Error('Expected recorded shortcut to be Ctrl+Shift+H, got: ' + hudKeyBtn.textContent);
}
const kbMsg = [...ipcMessages].reverse().find(m => m.type === 'update_keybinding');
if (!kbMsg || kbMsg.action !== 'toggle_fps_hud' || kbMsg.shortcut !== 'Ctrl+Shift+H') {
    throw new Error('Expected update_keybinding IPC for toggle_fps_hud=Ctrl+Shift+H, got: ' + JSON.stringify(kbMsg));
}

// 4. Restore Defaults button resets shortcuts to F2/F3/F5/F9/F10/F11 and emits restore_default_keybindings
const restoreBtn = shadow.getElementById('blockidle-restore-defaults');
if (!restoreBtn) {
    throw new Error('Missing #blockidle-restore-defaults button');
}
restoreBtn.click();
if (hudKeyBtn.textContent !== 'F3') {
    throw new Error('Expected Restore Defaults to reset toggle_fps_hud button to F3, got: ' + hudKeyBtn.textContent);
}
if (!ipcMessages.some(m => m.type === 'restore_default_keybindings')) {
    throw new Error('Expected restore_default_keybindings IPC message');
}

// 5. Pressing Escape when not recording closes the Settings Overlay without leaking Escape to the game
dispatchKey('keydown', 'Escape');
dispatchKey('keyup', 'Escape');
if (overlay.style.display !== 'none') {
    throw new Error('Expected Escape to close Settings Overlay when not recording');
}
if (gameKeyEvents.length !== 0) {
    throw new Error('Expected Escape closing overlay not to leak to game, got: ' + JSON.stringify(gameKeyEvents));
}

console.log(JSON.stringify(ipcMessages));
"#;

    let stdout = run_node_with_dom("settings-overlay", &init_script, test_body);

    // Feed emitted IPC messages from the overlay into ClientController and verify end-to-end state & persistence
    let last_line = stdout.lines().last().expect("json line");
    let messages: Vec<serde_json::Value> = serde_json::from_str(last_line).expect("parse ipc list");
    for msg in messages {
        let _ = controller.handle_ipc_message(&msg.to_string());
    }

    assert!(!controller.is_settings_overlay_open());
    assert_eq!(controller.settings().foreground_fps_cap, ForegroundFpsCap::Fps144);
    assert_eq!(controller.settings().background_fps_cap, BackgroundFpsCap::Fps0);
    assert!(controller.settings().fps_hud_visible);
    assert!(!controller.settings().keep_awake);
    assert!(controller.settings().close_to_tray);
    assert!(controller.settings().audio_muted);
    assert!(controller.settings().fullscreen);
    assert_eq!(controller.settings().keybindings, Keybindings::default());
}

#[test]
fn ensures_fontconfig_hintslight_and_mounts_webview_compat_style_for_fonts_and_select_contrast() {
    let dir = tempdir().expect("tempdir");
    let controller = ClientController::load_or_default(dir.path());

    let fonts_conf = controller.ensure_fontconfig_file().expect("fonts.conf");
    assert!(fonts_conf.exists());
    let xml = std::fs::read_to_string(&fonts_conf).expect("read fonts.conf");
    assert!(xml.contains("<const>hintslight</const>"));
    assert!(xml.contains("<edit name=\"hinting\" mode=\"assign\"><bool>true</bool></edit>"));
    assert!(xml.contains("<edit name=\"autohint\" mode=\"assign\"><bool>false</bool></edit>"));
    assert!(xml.contains("<edit name=\"rgba\" mode=\"assign\"><const>none</const></edit>"));

    let init_script = controller.initialization_script();
    let test_body = r#"
const compatStyle = document.getElementById('blockidle-webview-compat-style');
if (!compatStyle) {
    throw new Error('Expected #blockidle-webview-compat-style to be mounted');
}
const css = compatStyle.textContent;
if (!css.includes('color-scheme: light !important')) {
    throw new Error('Expected root color-scheme: light !important');
}
if (!css.includes('font-synthesis: none !important')) {
    throw new Error('Expected font-synthesis: none !important for Monocraft');
}
if (!css.includes('.painel, .vidro, #dicabox') || !css.includes('-webkit-font-smoothing: antialiased !important')) {
    throw new Error('Expected .painel, .vidro, #dicabox anti-aliasing override for WebKitGTK');
}
if (!css.includes('select option') || !css.includes('appearance: none !important')) {
    throw new Error('Expected select and select option contrast rules');
}
if (!css.includes('@media (max-height: 1120px)')) {
    throw new Error('Expected #menu two-column media query for 1080p desktop viewport');
}
console.log('COMPAT_STYLE_OK');
"#;
    let stdout = run_node_with_dom("compat-style", &init_script, test_body);
    assert!(stdout.contains("COMPAT_STYLE_OK"));
}

