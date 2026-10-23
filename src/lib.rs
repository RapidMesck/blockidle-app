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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayMenuState {
    pub window_visible: bool,
    pub audio_muted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ToggleWindowVisibility,
    Show,
    Hide,
    ToggleMute,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformEffect {
    OpenExternalBrowser(String),
    SyncWebViewState(String),
    SetWindowVisible(bool),
    SetFullscreen(bool),
    ReloadGameView,
    UpdateTrayMenu(TrayMenuState),
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
    ToggleAudioMute,
    HideToTray,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebViewClientState {
    pub settings: ClientSettings,
    pub window_focused: bool,
    pub window_visible: bool,
    pub settings_overlay_open: bool,
    pub active_fps_cap: Option<u32>,
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
    pub fn target_fps(self) -> Option<u32> {
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
    pub fn target_fps(self) -> u32 {
        match self {
            Self::Fps0 => 0,
            Self::Fps5 => 5,
            Self::Fps15 => 15,
            Self::Fps30 => 30,
            Self::Fps60 => 60,
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
    window_visible: bool,
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
            window_visible: true,
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

    pub fn is_window_visible(&self) -> bool {
        self.window_visible
    }

    pub fn active_fps_cap(&self) -> Option<u32> {
        if self.window_focused && self.window_visible {
            self.settings.foreground_fps_cap.target_fps()
        } else {
            Some(self.settings.background_fps_cap.target_fps())
        }
    }

    pub fn tray_menu_state(&self) -> TrayMenuState {
        TrayMenuState {
            window_visible: self.window_visible,
            audio_muted: self.settings.audio_muted,
        }
    }

    pub fn webview_state(&self) -> WebViewClientState {
        WebViewClientState {
            settings: self.settings.clone(),
            window_focused: self.window_focused,
            window_visible: self.window_visible,
            settings_overlay_open: self.settings_overlay_open,
            active_fps_cap: self.active_fps_cap(),
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

    function sendIpc(message) {{
        if (window.ipc && typeof window.ipc.postMessage === 'function') {{
            window.ipc.postMessage(JSON.stringify(message));
        }}
    }}

    function isAudioMuted() {{
        var st = window.__BLOCKIDLE_CLIENT_STATE__;
        return Boolean(st && st.settings && st.settings.audio_muted);
    }}

    var trackedAudioContexts = [];
    var trackedMediaElements = new Set();

    function wrapAudioContextConstructor(ctorName) {{
        var NativeAudioContext = window[ctorName];
        if (typeof NativeAudioContext !== 'function') {{
            return;
        }}
        function WrappedAudioContext() {{
            var ctx = Reflect.construct(NativeAudioContext, arguments, new.target || WrappedAudioContext);
            var nativeResume = typeof ctx.resume === 'function' ? ctx.resume.bind(ctx) : null;
            var nativeSuspend = typeof ctx.suspend === 'function' ? ctx.suspend.bind(ctx) : null;
            ctx.__blockidleNativeResume = nativeResume;
            ctx.__blockidleNativeSuspend = nativeSuspend;
            if (nativeResume) {{
                ctx.resume = function() {{
                    if (isAudioMuted()) {{
                        if (nativeSuspend) {{
                            return nativeSuspend();
                        }}
                        return Promise.resolve();
                    }}
                    return nativeResume();
                }};
            }}
            trackedAudioContexts.push(ctx);
            if (isAudioMuted() && nativeSuspend) {{
                try {{ nativeSuspend(); }} catch (_) {{}}
            }}
            return ctx;
        }}
        WrappedAudioContext.prototype = NativeAudioContext.prototype;
        Object.setPrototypeOf(WrappedAudioContext, NativeAudioContext);
        window[ctorName] = WrappedAudioContext;
    }}

    wrapAudioContextConstructor('AudioContext');
    wrapAudioContextConstructor('webkitAudioContext');

    if (typeof window.HTMLMediaElement !== 'undefined' && window.HTMLMediaElement.prototype) {{
        var nativePlay = window.HTMLMediaElement.prototype.play;
        if (typeof nativePlay === 'function') {{
            window.HTMLMediaElement.prototype.play = function() {{
                trackedMediaElements.add(this);
                if (isAudioMuted()) {{
                    this.muted = true;
                }}
                return nativePlay.apply(this, arguments);
            }};
        }}
    }}

    function syncAudioMuteState() {{
        var muted = isAudioMuted();
        for (var i = 0; i < trackedAudioContexts.length; i++) {{
            var ctx = trackedAudioContexts[i];
            if (!ctx || ctx.state === 'closed') {{
                continue;
            }}
            try {{
                if (muted && typeof ctx.__blockidleNativeSuspend === 'function') {{
                    ctx.__blockidleNativeSuspend();
                }} else if (!muted && typeof ctx.__blockidleNativeResume === 'function') {{
                    ctx.__blockidleNativeResume();
                }}
            }} catch (_) {{}}
        }}
        if (typeof document !== 'undefined' && typeof document.querySelectorAll === 'function') {{
            var domMedia = document.querySelectorAll('audio, video');
            for (var j = 0; j < domMedia.length; j++) {{
                trackedMediaElements.add(domMedia[j]);
            }}
        }}
        trackedMediaElements.forEach(function(media) {{
            if (media) {{
                media.muted = muted;
            }}
        }});
    }}

    window.__blockidleApplyState = function(nextState) {{
        window.__BLOCKIDLE_CLIENT_STATE__ = nextState;
        syncAudioMuteState();
        window.dispatchEvent(new CustomEvent('blockidle:state', {{ detail: nextState }}));
    }};

    syncAudioMuteState();

    window.__BLOCKIDLE_CLIENT__ = {{
        getState: function() {{ return window.__BLOCKIDLE_CLIENT_STATE__; }},
        sendIpc: sendIpc,
        applyState: window.__blockidleApplyState
    }};

    if (document.readyState === 'loading') {{
        document.addEventListener('DOMContentLoaded', function() {{
            syncAudioMuteState();
            sendIpc({{ type: 'client_ready' }});
        }}, {{ once: true }});
    }} else {{
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
                let audio_muted_changed = self.settings.audio_muted != settings.audio_muted;
                let _ = self.update_settings(settings);
                let mut effects = Vec::new();
                if fullscreen_changed {
                    effects.push(PlatformEffect::SetFullscreen(fullscreen_target));
                }
                if audio_muted_changed {
                    effects.push(PlatformEffect::UpdateTrayMenu(self.tray_menu_state()));
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
            IpcMessage::ToggleAudioMute => self.toggle_audio_mute(),
            IpcMessage::HideToTray => self.hide_to_tray(),
        }
    }

    pub fn handle_window_resized(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 && !self.settings.fullscreen {
            self.settings.window_width = width;
            self.settings.window_height = height;
            let _ = self.save_settings();
        }
    }

    pub fn handle_focus_changed(&mut self, focused: bool) -> Vec<PlatformEffect> {
        self.window_focused = focused;
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn hide_to_tray(&mut self) -> Vec<PlatformEffect> {
        self.window_visible = false;
        self.window_focused = false;
        vec![
            PlatformEffect::SetWindowVisible(false),
            PlatformEffect::UpdateTrayMenu(self.tray_menu_state()),
            PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
        ]
    }

    pub fn restore_from_tray(&mut self) -> Vec<PlatformEffect> {
        self.window_visible = true;
        self.window_focused = true;
        vec![
            PlatformEffect::SetWindowVisible(true),
            PlatformEffect::UpdateTrayMenu(self.tray_menu_state()),
            PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
        ]
    }

    pub fn toggle_audio_mute(&mut self) -> Vec<PlatformEffect> {
        self.settings.audio_muted = !self.settings.audio_muted;
        let _ = self.save_settings();
        vec![
            PlatformEffect::UpdateTrayMenu(self.tray_menu_state()),
            PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
        ]
    }

    pub fn handle_tray_action(&mut self, action: TrayAction) -> Vec<PlatformEffect> {
        match action {
            TrayAction::Hide => self.hide_to_tray(),
            TrayAction::Show => self.restore_from_tray(),
            TrayAction::ToggleWindowVisibility => {
                if self.window_visible {
                    self.hide_to_tray()
                } else {
                    self.restore_from_tray()
                }
            }
            TrayAction::ToggleMute => self.toggle_audio_mute(),
            TrayAction::Quit => vec![PlatformEffect::ExitApp],
        }
    }

    pub fn handle_close_requested(&mut self) -> Vec<PlatformEffect> {
        if self.settings.close_to_tray {
            self.hide_to_tray()
        } else {
            vec![PlatformEffect::ExitApp]
        }
    }
}
