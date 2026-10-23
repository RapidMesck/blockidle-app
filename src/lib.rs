use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use url::Url;

pub const GAME_VIEW_INITIAL_URL: &str = "https://blockidle.io/play";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationDecision {
    AllowInGameView,
    OpenExternalBrowser(String),
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEffect {
    OpenExternalBrowser(String),
    SyncWebViewState(String),
    SetWindowVisible(bool),
    SetFullscreen(bool),
    ReloadGameView,
    ExitApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    ClientReady,
    UpdateSettings { settings: ClientSettings },
    OpenExternalUrl { url: String },
    ReloadGameView,
    ToggleFullscreen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebViewClientState {
    pub settings: ClientSettings,
    pub window_focused: bool,
    pub window_minimized: bool,
    pub effective_fps_cap: Option<u32>,
    pub settings_overlay_open: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForegroundFpsCap {
    Fps15,
    Fps30,
    Fps60,
    Fps120,
    Fps144,
    Fps240,
    Unlimited,
}

impl ForegroundFpsCap {
    pub fn as_fps(self) -> Option<u32> {
        match self {
            Self::Fps15 => Some(15),
            Self::Fps30 => Some(30),
            Self::Fps60 => Some(60),
            Self::Fps120 => Some(120),
            Self::Fps144 => Some(144),
            Self::Fps240 => Some(240),
            Self::Unlimited => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundFpsCap {
    Fps0,
    Fps5,
    Fps15,
    Fps30,
    Fps60,
}

impl BackgroundFpsCap {
    pub fn as_fps(self) -> Option<u32> {
        match self {
            Self::Fps0 => Some(0),
            Self::Fps5 => Some(5),
            Self::Fps15 => Some(15),
            Self::Fps30 => Some(30),
            Self::Fps60 => Some(60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Keybindings {
    pub toggle_settings_overlay: String,
    pub toggle_fps_hud: String,
    pub reload_game_view: String,
    pub hide_to_tray: String,
    pub toggle_audio_mute: String,
    pub toggle_fullscreen: String,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            toggle_settings_overlay: "F2".to_string(),
            toggle_fps_hud: "F3".to_string(),
            reload_game_view: "F5".to_string(),
            hide_to_tray: "F9".to_string(),
            toggle_audio_mute: "F10".to_string(),
            toggle_fullscreen: "F11".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientSettings {
    pub foreground_fps_cap: ForegroundFpsCap,
    pub background_fps_cap: BackgroundFpsCap,
    pub fps_hud_visible: bool,
    pub keep_awake: bool,
    pub close_to_tray: bool,
    pub audio_muted: bool,
    pub fullscreen: bool,
    pub window_width: u32,
    pub window_height: u32,
    pub keybindings: Keybindings,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            foreground_fps_cap: ForegroundFpsCap::Unlimited,
            background_fps_cap: BackgroundFpsCap::Fps15,
            fps_hud_visible: false,
            keep_awake: true,
            close_to_tray: false,
            audio_muted: false,
            fullscreen: false,
            window_width: 1280,
            window_height: 720,
            keybindings: Keybindings::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Option<Self> {
        let proj = directories::ProjectDirs::from("io", "blockidle", "Blockidle")?;
        Some(Self {
            config_dir: proj.config_dir().to_path_buf(),
            data_dir: proj.data_dir().to_path_buf(),
        })
    }
}

pub struct ClientController {
    config_path: PathBuf,
    data_dir: PathBuf,
    settings: ClientSettings,
    window_focused: bool,
    window_minimized: bool,
    settings_overlay_open: bool,
}

impl ClientController {
    pub fn load_or_default(config_dir: impl AsRef<Path>) -> Self {
        let dir = config_dir.as_ref();
        Self::with_paths(dir, dir.join("webview-data"))
    }

    pub fn with_paths(config_dir: impl AsRef<Path>, data_dir: impl AsRef<Path>) -> Self {
        let config_path = config_dir.as_ref().join("settings.json");
        let settings = fs::read_to_string(&config_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<ClientSettings>(&raw).ok())
            .unwrap_or_default();

        Self {
            config_path,
            data_dir: data_dir.as_ref().to_path_buf(),
            settings,
            window_focused: true,
            window_minimized: false,
            settings_overlay_open: false,
        }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn game_view_url(&self) -> &'static str {
        GAME_VIEW_INITIAL_URL
    }

    pub fn settings(&self) -> &ClientSettings {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: ClientSettings) -> io::Result<()> {
        self.settings = settings;
        self.save_settings()
    }

    pub fn save_settings(&self) -> io::Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.settings)
            .map_err(io::Error::other)?;
        fs::write(&self.config_path, json)
    }

    pub fn handle_navigation_request(
        &mut self,
        url: &str,
    ) -> (NavigationDecision, Vec<PlatformEffect>) {
        if url == "about:blank" {
            return (NavigationDecision::AllowInGameView, Vec::new());
        }

        let Ok(parsed) = Url::parse(url) else {
            return (NavigationDecision::Block, Vec::new());
        };

        match parsed.scheme() {
            "http" | "https" => {}
            _ => return (NavigationDecision::Block, Vec::new()),
        }

        let Some(host) = parsed.host_str() else {
            return (NavigationDecision::Block, Vec::new());
        };

        let host = host.to_ascii_lowercase();
        let allowed_in_game_view = host == "blockidle.io"
            || host.ends_with(".blockidle.io")
            || host == "challenges.cloudflare.com"
            || host == "stripe.com"
            || host.ends_with(".stripe.com");

        if allowed_in_game_view {
            (NavigationDecision::AllowInGameView, Vec::new())
        } else {
            let target = parsed.to_string();
            (
                NavigationDecision::OpenExternalBrowser(url.to_string()),
                vec![PlatformEffect::OpenExternalBrowser(if target.ends_with('/') && !url.ends_with('/') {
                    url.to_string()
                } else {
                    url.to_string()
                })],
            )
        }
    }

    pub fn handle_new_window_request(
        &mut self,
        url: &str,
    ) -> (NavigationDecision, Vec<PlatformEffect>) {
        let Ok(parsed) = Url::parse(url) else {
            return (NavigationDecision::Block, Vec::new());
        };

        match parsed.scheme() {
            "http" | "https" => (
                NavigationDecision::OpenExternalBrowser(url.to_string()),
                vec![PlatformEffect::OpenExternalBrowser(url.to_string())],
            ),
            _ => (NavigationDecision::Block, Vec::new()),
        }
    }

    pub fn effective_fps_cap(&self) -> Option<u32> {
        if self.window_focused && !self.window_minimized {
            self.settings.foreground_fps_cap.as_fps()
        } else {
            self.settings.background_fps_cap.as_fps()
        }
    }

    pub fn webview_state(&self) -> WebViewClientState {
        WebViewClientState {
            settings: self.settings.clone(),
            window_focused: self.window_focused,
            window_minimized: self.window_minimized,
            effective_fps_cap: self.effective_fps_cap(),
            settings_overlay_open: self.settings_overlay_open,
        }
    }

    pub fn webview_state_sync_script(&self) -> String {
        let state_json =
            serde_json::to_string(&self.webview_state()).unwrap_or_else(|_| "{}".to_string());
        format!(
            "if (typeof window.__blockidleApplyState === 'function') {{ window.__blockidleApplyState({state_json}); }}"
        )
    }

    pub fn initialization_script(&self) -> String {
        let state_json =
            serde_json::to_string(&self.webview_state()).unwrap_or_else(|_| "{}".to_string());
        format!(
            r#"(function() {{
    'use strict';
    if (window.__BLOCKIDLE_CLIENT_INITIALIZED__) {{
        return;
    }}
    window.__BLOCKIDLE_CLIENT_INITIALIZED__ = true;
    window.__BLOCKIDLE_CLIENT_STATE__ = {state_json};

    const nativeRaf = window.requestAnimationFrame
        ? window.requestAnimationFrame.bind(window)
        : function(cb) {{ return window.setTimeout(function() {{ cb(performance.now()); }}, 16); }};
    const nativeCancelRaf = window.cancelAnimationFrame
        ? window.cancelAnimationFrame.bind(window)
        : function(id) {{ window.clearTimeout(id); }};
    const nativeSetTimeout = window.setTimeout.bind(window);
    const nativeClearTimeout = window.clearTimeout.bind(window);

    let nextRequestId = 1;
    let pendingCallbacks = [];
    let scheduledNativeRafId = null;
    let scheduledTimerId = null;
    let lastFrameTimestamp = -Infinity;
    let deliveredFrameTimestamps = [];
    let liveFps = 0;

    function isKeepAwakeEnabled() {{
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        return Boolean(st && st.settings && st.settings.keep_awake);
    }}

    const docProto = (typeof Document !== 'undefined' && Document.prototype)
        ? Document.prototype
        : Object.getPrototypeOf(document);
    const nativeHiddenDescriptor =
        Object.getOwnPropertyDescriptor(document, 'hidden') ||
        (docProto ? Object.getOwnPropertyDescriptor(docProto, 'hidden') : null);
    const nativeVisibilityStateDescriptor =
        Object.getOwnPropertyDescriptor(document, 'visibilityState') ||
        (docProto ? Object.getOwnPropertyDescriptor(docProto, 'visibilityState') : null);

    try {{
        Object.defineProperty(document, 'hidden', {{
            configurable: true,
            enumerable: true,
            get: function() {{
                if (isKeepAwakeEnabled()) {{
                    return false;
                }}
                if (nativeHiddenDescriptor && typeof nativeHiddenDescriptor.get === 'function') {{
                    return nativeHiddenDescriptor.get.call(document);
                }}
                return false;
            }}
        }});
        Object.defineProperty(document, 'visibilityState', {{
            configurable: true,
            enumerable: true,
            get: function() {{
                if (isKeepAwakeEnabled()) {{
                    return 'visible';
                }}
                if (nativeVisibilityStateDescriptor && typeof nativeVisibilityStateDescriptor.get === 'function') {{
                    return nativeVisibilityStateDescriptor.get.call(document);
                }}
                return 'visible';
            }}
        }});
    }} catch (_err) {{}}

    function suppressVisibilityChange(event) {{
        if (isKeepAwakeEnabled()) {{
            if (event && typeof event.stopImmediatePropagation === 'function') {{
                event.stopImmediatePropagation();
            }}
            if (event && typeof event.stopPropagation === 'function') {{
                event.stopPropagation();
            }}
        }}
    }}
    document.addEventListener('visibilitychange', suppressVisibilityChange, true);
    window.addEventListener('visibilitychange', suppressVisibilityChange, true);

    function getEffectiveCap() {{
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        if (!st) {{
            return null;
        }}
        if (typeof st.effective_fps_cap === 'number') {{
            return st.effective_fps_cap;
        }}
        return null;
    }}

    let hudElement = null;

    function formatCapLabel(cap) {{
        if (cap === null || cap === undefined) {{
            return 'Unlimited';
        }}
        if (cap === 0) {{
            return '0 FPS (Paused)';
        }}
        return String(cap) + ' FPS';
    }}

    function updateFpsHud() {{
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const visible = Boolean(st && st.settings && st.settings.fps_hud_visible);
        if (!hudElement) {{
            ensureOverlayMounted();
        }}
        if (!hudElement) {{
            return;
        }}
        hudElement.style.display = visible ? 'block' : 'none';
        if (!visible) {{
            return;
        }}
        const cap = getEffectiveCap();
        const fps = getLiveFps();
        hudElement.textContent = fps + ' FPS | Cap: ' + formatCapLabel(cap);
    }}

    function ensureOverlayMounted() {{
        if (hudElement) {{
            return;
        }}
        const rootTarget = document.body || document.documentElement;
        if (!rootTarget) {{
            return;
        }}
        let host = document.getElementById('blockidle-client-overlay-host');
        if (!host) {{
            host = document.createElement('div');
            host.id = 'blockidle-client-overlay-host';
            host.style.position = 'fixed';
            host.style.top = '0';
            host.style.left = '0';
            host.style.width = '100%';
            host.style.height = '100%';
            host.style.pointerEvents = 'none';
            host.style.zIndex = '2147483647';
            rootTarget.appendChild(host);
        }}
        const shadow = host.shadowRoot || host.attachShadow({{ mode: 'open' }});
        let styleEl = shadow.getElementById ? shadow.getElementById('blockidle-overlay-style') : null;
        if (!styleEl) {{
            styleEl = document.createElement('style');
            styleEl.id = 'blockidle-overlay-style';
            styleEl.textContent =
                '#blockidle-fps-hud {{ position: fixed; top: 12px; left: 12px; padding: 6px 10px; ' +
                'background: rgba(15, 18, 24, 0.78); color: #e6edf3; font-family: monospace; ' +
                'font-size: 12px; font-weight: 600; border-radius: 6px; border: 1px solid rgba(255, 255, 255, 0.14); ' +
                'pointer-events: none; user-select: none; z-index: 2147483647; }}';
            shadow.appendChild(styleEl);
        }}
        let hud = shadow.getElementById ? shadow.getElementById('blockidle-fps-hud') : null;
        if (!hud) {{
            hud = document.createElement('div');
            hud.id = 'blockidle-fps-hud';
            hud.style.pointerEvents = 'none';
            shadow.appendChild(hud);
        }}
        hudElement = hud;
        updateFpsHud();
    }}

    function recordDeliveredFrame(now) {{
        deliveredFrameTimestamps.push(now);
        const cutoff = now - 1000;
        while (deliveredFrameTimestamps.length > 0 && deliveredFrameTimestamps[0] <= cutoff) {{
            deliveredFrameTimestamps.shift();
        }}
        liveFps = deliveredFrameTimestamps.length;
        updateFpsHud();
    }}

    function getLiveFps() {{
        const now = (window.performance && typeof window.performance.now === 'function')
            ? window.performance.now()
            : Date.now();
        const cutoff = now - 1000;
        while (deliveredFrameTimestamps.length > 0 && deliveredFrameTimestamps[0] <= cutoff) {{
            deliveredFrameTimestamps.shift();
        }}
        if ( getEffectiveCap() === 0 ) {{
            return 0;
        }}
        liveFps = deliveredFrameTimestamps.length;
        return liveFps;
    }}

    function clearScheduledPump() {{
        if (scheduledNativeRafId !== null) {{
            nativeCancelRaf(scheduledNativeRafId);
            scheduledNativeRafId = null;
        }}
        if (scheduledTimerId !== null) {{
            nativeClearTimeout(scheduledTimerId);
            scheduledTimerId = null;
        }}
    }}

    function flushCallbacks(timestamp) {{
        const batch = pendingCallbacks.splice(0, pendingCallbacks.length);
        if (batch.length === 0) {{
            return;
        }}
        recordDeliveredFrame(timestamp);
        for (let i = 0; i < batch.length; i++) {{
            try {{
                batch[i].callback(timestamp);
            }} catch (err) {{
                nativeSetTimeout(function() {{ throw err; }}, 0);
            }}
        }}
    }}

    function schedulePump() {{
        if (pendingCallbacks.length === 0) {{
            return;
        }}
        const cap = getEffectiveCap();
        if (cap === 0) {{
            clearScheduledPump();
            return;
        }}
        if (scheduledNativeRafId !== null || scheduledTimerId !== null) {{
            return;
        }}
        if (cap === null || cap === undefined) {{
            scheduledNativeRafId = nativeRaf(function(ts) {{
                scheduledNativeRafId = null;
                lastFrameTimestamp = ts;
                flushCallbacks(ts);
                if (pendingCallbacks.length > 0) {{
                    schedulePump();
                }}
            }});
            return;
        }}

        const interval = 1000 / cap;
        const now = (window.performance && typeof window.performance.now === 'function')
            ? window.performance.now()
            : Date.now();
        const remaining = (lastFrameTimestamp + interval) - now;

        if (remaining <= 0.5) {{
            scheduledNativeRafId = nativeRaf(function(ts) {{
                scheduledNativeRafId = null;
                const currentCap = getEffectiveCap();
                if (currentCap === 0) {{
                    return;
                }}
                if (currentCap !== null && currentCap !== undefined) {{
                    const targetInterval = 1000 / currentCap;
                    if (ts - lastFrameTimestamp < targetInterval - 0.5) {{
                        schedulePump();
                        return;
                    }}
                    if (!Number.isFinite(lastFrameTimestamp) || (ts - lastFrameTimestamp) > targetInterval * 2) {{
                        lastFrameTimestamp = ts;
                    }} else {{
                        lastFrameTimestamp = lastFrameTimestamp + targetInterval;
                    }}
                }} else {{
                    lastFrameTimestamp = ts;
                }}
                flushCallbacks(ts);
                if (pendingCallbacks.length > 0) {{
                    schedulePump();
                }}
            }});
        }} else {{
            scheduledTimerId = nativeSetTimeout(function() {{
                scheduledTimerId = null;
                schedulePump();
            }}, Math.max(0, Math.floor(remaining)));
        }}
    }}

    window.requestAnimationFrame = function(callback) {{
        if (typeof callback !== 'function') {{
            throw new TypeError('requestAnimationFrame callback must be a function');
        }}
        const id = nextRequestId++;
        pendingCallbacks.push({{ id: id, callback: callback }});
        schedulePump();
        return id;
    }};

    window.cancelAnimationFrame = function(id) {{
        pendingCallbacks = pendingCallbacks.filter(function(entry) {{
            return entry.id !== id;
        }});
        if (pendingCallbacks.length === 0) {{
            clearScheduledPump();
        }}
    }};

    function sendIpc(message) {{
        if (window.ipc && typeof window.ipc.postMessage === 'function') {{
            window.ipc.postMessage(JSON.stringify(message));
        }}
    }}

    window.__blockidleApplyState = function(nextState) {{
        const prevCap = getEffectiveCap();
        window.__BLOCKIDLE_CLIENT_STATE__ = nextState;
        const nextCap = getEffectiveCap();
        if (nextCap === 0) {{
            clearScheduledPump();
        }} else if (prevCap === 0 && (nextCap === null || nextCap > 0)) {{
            lastFrameTimestamp = -Infinity;
            clearScheduledPump();
            schedulePump();
        }} else if (prevCap !== nextCap) {{
            clearScheduledPump();
            schedulePump();
        }}
        updateFpsHud();
        window.dispatchEvent(new CustomEvent('blockidle:state', {{ detail: nextState }}));
    }};

    window.__BLOCKIDLE_CLIENT__ = {{
        getState: function() {{ return window.__BLOCKIDLE_CLIENT_STATE__; }},
        getLiveFps: getLiveFps,
        sendIpc: sendIpc,
        applyState: window.__blockidleApplyState
    }};

    if (typeof window.setInterval === 'function') {{
        window.setInterval(updateFpsHud, 250);
    }}

    if (document.readyState === 'loading') {{
        document.addEventListener('DOMContentLoaded', function() {{
            ensureOverlayMounted();
            sendIpc({{ type: 'client_ready' }});
        }}, {{ once: true }});
    }} else {{
        ensureOverlayMounted();
        sendIpc({{ type: 'client_ready' }});
    }}
}})();"#
        )
    }

    pub fn handle_ipc_message(&mut self, raw_json: &str) -> Vec<PlatformEffect> {
        let Ok(message) = serde_json::from_str::<IpcMessage>(raw_json) else {
            return Vec::new();
        };

        match message {
            IpcMessage::ClientReady => {
                vec![PlatformEffect::SyncWebViewState(
                    self.webview_state_sync_script(),
                )]
            }
            IpcMessage::UpdateSettings { settings } => {
                let fullscreen_changed = self.settings.fullscreen != settings.fullscreen;
                let fullscreen_target = settings.fullscreen;
                let _ = self.update_settings(settings);
                let mut effects = Vec::new();
                if fullscreen_changed {
                    effects.push(PlatformEffect::SetFullscreen(fullscreen_target));
                }
                effects.push(PlatformEffect::SyncWebViewState(
                    self.webview_state_sync_script(),
                ));
                effects
            }
            IpcMessage::OpenExternalUrl { url } => {
                let (_, effects) = self.handle_new_window_request(&url);
                effects
            }
            IpcMessage::ReloadGameView => vec![PlatformEffect::ReloadGameView],
            IpcMessage::ToggleFullscreen => {
                self.settings.fullscreen = !self.settings.fullscreen;
                let _ = self.save_settings();
                vec![
                    PlatformEffect::SetFullscreen(self.settings.fullscreen),
                    PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
                ]
            }
        }
    }

    pub fn handle_window_resized(&mut self, width: u32, height: u32) -> Vec<PlatformEffect> {
        if width == 0 || height == 0 {
            return self.handle_minimized_changed(true);
        }

        if !self.settings.fullscreen {
            self.settings.window_width = width;
            self.settings.window_height = height;
            let _ = self.save_settings();
        }

        let mut effects = Vec::new();
        if self.window_minimized {
            self.window_minimized = false;
            effects.push(PlatformEffect::SyncWebViewState(
                self.webview_state_sync_script(),
            ));
        }
        effects
    }

    pub fn handle_minimized_changed(&mut self, minimized: bool) -> Vec<PlatformEffect> {
        self.window_minimized = minimized;
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn handle_focus_changed(&mut self, focused: bool) -> Vec<PlatformEffect> {
        self.window_focused = focused;
        if focused {
            self.window_minimized = false;
        }
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn handle_close_requested(&mut self) -> Vec<PlatformEffect> {
        if self.settings.close_to_tray {
            self.window_focused = false;
            vec![
                PlatformEffect::SetWindowVisible(false),
                PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
            ]
        } else {
            vec![PlatformEffect::ExitApp]
        }
    }
}
