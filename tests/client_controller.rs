use blockidle_app::{
    BackgroundFpsCap, ClientController, ClientSettings, ForegroundFpsCap, Keybindings,
    NavigationDecision, PlatformEffect,
};
use tempfile::tempdir;

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
    assert!(os_paths.config_dir.ends_with("blockidle") || os_paths.config_dir.ends_with("Blockidle") || !os_paths.config_dir.as_os_str().is_empty());
    assert!(!os_paths.data_dir.as_os_str().is_empty());
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

    let test_harness = format!(
        r#"
let now = 1000;
let nativeRafCallbacks = [];
let nextNativeRafId = 1;
let timers = [];
let nextTimerId = 1;

const window = {{
    performance: {{ now: () => now }},
    requestAnimationFrame: (cb) => {{
        const id = nextNativeRafId++;
        nativeRafCallbacks.push({{ id, cb }});
        return id;
    }},
    cancelAnimationFrame: (id) => {{
        nativeRafCallbacks = nativeRafCallbacks.filter(item => item.id !== id);
    }},
    setTimeout: (cb, delay) => {{
        const id = nextTimerId++;
        timers.push({{ id, cb, runAt: now + Math.max(0, delay || 0) }});
        return id;
    }},
    clearTimeout: (id) => {{
        timers = timers.filter(item => item.id !== id);
    }},
    addEventListener: () => {{}},
    dispatchEvent: () => true,
    CustomEvent: class CustomEvent {{ constructor(type, opts) {{ this.type = type; this.detail = opts ? opts.detail : undefined; }} }},
    ipc: {{ postMessage: () => {{}} }}
}};
globalThis.window = window;
globalThis.performance = window.performance;
globalThis.requestAnimationFrame = (...args) => window.requestAnimationFrame(...args);
globalThis.cancelAnimationFrame = (...args) => window.cancelAnimationFrame(...args);
globalThis.setTimeout = window.setTimeout;
globalThis.clearTimeout = window.clearTimeout;
globalThis.CustomEvent = window.CustomEvent;

const mockShadowRoot = {{
    innerHTML: '',
    appendChild: () => {{}},
    getElementById: () => null,
    querySelector: () => null,
}};
const mockElement = () => ({{
    id: '',
    style: {{}},
    textContent: '',
    innerHTML: '',
    attachShadow: () => mockShadowRoot,
    appendChild: () => {{}},
    setAttribute: () => {{}},
}});
const document = {{
    readyState: 'complete',
    hidden: false,
    visibilityState: 'visible',
    addEventListener: () => {{}},
    createElement: mockElement,
    getElementById: () => null,
    documentElement: mockElement(),
    body: mockElement(),
}};
globalThis.document = document;
window.document = document;

function stepNativeFrame(dtMs) {{
    now += dtMs;
    const dueTimers = timers.filter(t => t.runAt <= now);
    timers = timers.filter(t => t.runAt > now);
    for (const t of dueTimers) {{
        t.cb();
    }}
    const batch = nativeRafCallbacks.slice();
    nativeRafCallbacks = [];
    for (const item of batch) {{
        item.cb(now);
    }}
}}

{init_script}

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

    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(&test_harness)
        .output()
        .expect("run node to verify initialization script");
    assert!(
        output.status.success(),
        "node test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

    let test_harness = format!(
        r#"
let nativeHidden = false;
let nativeVisibilityState = 'visible';
const docListeners = [];

const document = {{
    readyState: 'complete',
    get hidden() {{ return nativeHidden; }},
    get visibilityState() {{ return nativeVisibilityState; }},
    addEventListener(type, listener, options) {{
        const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
        docListeners.push({{ type, listener, capture }});
    }},
    dispatchVisibilityChange() {{
        let stopped = false;
        const event = {{
            type: 'visibilitychange',
            stopImmediatePropagation() {{ stopped = true; }},
            stopPropagation() {{ stopped = true; }},
            preventDefault() {{}}
        }};
        const sorted = docListeners
            .filter(l => l.type === 'visibilitychange')
            .sort((a, b) => (b.capture ? 1 : 0) - (a.capture ? 1 : 0));
        for (const item of sorted) {{
            if (stopped) break;
            item.listener(event);
        }}
    }},
    createElement: () => ({{
        id: '',
        style: {{}},
        textContent: '',
        innerHTML: '',
        attachShadow: () => ({{ innerHTML: '', appendChild: () => {{}}, getElementById: () => null, querySelector: () => null }}),
        appendChild: () => {{}},
        setAttribute: () => {{}}
    }}),
    getElementById: () => null,
    documentElement: {{ appendChild: () => {{}} }},
    body: {{ appendChild: () => {{}} }}
}};

const window = {{
    performance: {{ now: () => 0 }},
    requestAnimationFrame: () => 1,
    cancelAnimationFrame: () => {{}},
    setTimeout: () => 1,
    clearTimeout: () => {{}},
    addEventListener: () => {{}},
    dispatchEvent: () => true,
    CustomEvent: class CustomEvent {{ constructor(type, opts) {{ this.type = type; this.detail = opts ? opts.detail : undefined; }} }},
    ipc: {{ postMessage: () => {{}} }},
    document: document
}};
globalThis.window = window;
globalThis.document = document;
globalThis.performance = window.performance;
globalThis.CustomEvent = window.CustomEvent;

{init_script}

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

    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(&test_harness)
        .output()
        .expect("run node to verify keep awake");
    assert!(
        output.status.success(),
        "node keep-awake test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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

    let test_harness = format!(
        r#"
let now = 1000;
let nativeRafCallbacks = [];
let nextNativeRafId = 1;

function createMockNode(tagName) {{
    const children = [];
    const elementsById = new Map();
    const node = {{
        tagName: tagName || 'DIV',
        id: '',
        style: {{}},
        textContent: '',
        innerHTML: '',
        children: children,
        shadowRoot: null,
        attachShadow(opts) {{
            const root = createMockNode('SHADOW-ROOT');
            root.mode = opts && opts.mode;
            this.shadowRoot = root;
            return root;
        }},
        appendChild(child) {{
            children.push(child);
            if (child && child.id) {{
                elementsById.set(child.id, child);
            }}
            return child;
        }},
        getElementById(id) {{
            if (elementsById.has(id)) return elementsById.get(id);
            for (const c of children) {{
                if (c.id === id) return c;
                if (c.getElementById) {{
                    const found = c.getElementById(id);
                    if (found) return found;
                }}
            }}
            return null;
        }},
        querySelector(sel) {{
            if (sel.startsWith('#')) {{
                return this.getElementById(sel.slice(1));
            }}
            return null;
        }},
        setAttribute() {{}}
    }};
    return node;
}}

const body = createMockNode('BODY');
const documentElement = createMockNode('HTML');
const document = {{
    readyState: 'complete',
    hidden: false,
    visibilityState: 'visible',
    body: body,
    documentElement: documentElement,
    addEventListener: () => {{}},
    createElement: (tag) => createMockNode(tag),
    getElementById: (id) => body.getElementById(id) || documentElement.getElementById(id),
}};

const window = {{
    performance: {{ now: () => now }},
    requestAnimationFrame: (cb) => {{
        const id = nextNativeRafId++;
        nativeRafCallbacks.push({{ id, cb }});
        return id;
    }},
    cancelAnimationFrame: (id) => {{
        nativeRafCallbacks = nativeRafCallbacks.filter(x => x.id !== id);
    }},
    setTimeout: () => 1,
    clearTimeout: () => {{}},
    setInterval: () => 1,
    clearInterval: () => {{}},
    addEventListener: () => {{}},
    dispatchEvent: () => true,
    CustomEvent: class CustomEvent {{ constructor(type, opts) {{ this.type = type; this.detail = opts ? opts.detail : undefined; }} }},
    ipc: {{ postMessage: () => {{}} }},
    document: document
}};
globalThis.window = window;
globalThis.document = document;
globalThis.performance = window.performance;
globalThis.CustomEvent = window.CustomEvent;

{init_script}

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
    now += 10;
    const batch = nativeRafCallbacks.slice();
    nativeRafCallbacks = [];
    for (const item of batch) item.cb(now);
}}
if (!hud.textContent.includes('30') || !hud.textContent.includes('Unlimited')) {{
    throw new Error('Expected FPS HUD to show 30 FPS and Unlimited cap, got: ' + hud.textContent);
}}
console.log('HUD_TEST_OK');
"#
    );

    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(&test_harness)
        .output()
        .expect("run node to verify shadow dom fps hud");
    assert!(
        output.status.success(),
        "node fps-hud test failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}






