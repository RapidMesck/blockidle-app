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
    UpdateKeybinding {
        action: KeybindingAction,
        shortcut: String,
    },
    RestoreDefaultKeybindings,
    TriggerKeybinding { shortcut: String },
    ToggleSettingsOverlay,
    SetSettingsOverlayOpen { open: bool },
    ToggleFpsHud,
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
    pub window_minimized: bool,
    pub effective_fps_cap: Option<u32>,
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

    pub fn target_fps(self) -> Option<u32> {
        self.as_fps()
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
        Some(self.target_fps())
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeybindingAction {
    ToggleSettingsOverlay,
    ToggleFpsHud,
    ReloadGameView,
    HideToTray,
    ToggleAudioMute,
    ToggleFullscreen,
}

impl KeybindingAction {
    pub const ALL: [Self; 6] = [
        Self::ToggleSettingsOverlay,
        Self::ToggleFpsHud,
        Self::ReloadGameView,
        Self::HideToTray,
        Self::ToggleAudioMute,
        Self::ToggleFullscreen,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeybindingError {
    InvalidShortcut(String),
    DuplicateShortcut {
        shortcut: String,
        existing_action: KeybindingAction,
    },
}

fn normalize_key_token(token: &str) -> Option<String> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    match lower.as_str() {
        "ctrl" | "control" | "alt" | "option" | "shift" | "meta" | "super" | "command" | "os"
        | "escape" | "esc" => return None,
        "enter" | "return" => return Some("Enter".to_string()),
        "space" | "spacebar" => return Some("Space".to_string()),
        "tab" => return Some("Tab".to_string()),
        "backspace" => return Some("Backspace".to_string()),
        "delete" | "del" => return Some("Delete".to_string()),
        "insert" | "ins" => return Some("Insert".to_string()),
        "home" => return Some("Home".to_string()),
        "end" => return Some("End".to_string()),
        "pageup" | "pgup" => return Some("PageUp".to_string()),
        "pagedown" | "pgdn" => return Some("PageDown".to_string()),
        "arrowup" | "up" => return Some("ArrowUp".to_string()),
        "arrowdown" | "down" => return Some("ArrowDown".to_string()),
        "arrowleft" | "left" => return Some("ArrowLeft".to_string()),
        "arrowright" | "right" => return Some("ArrowRight".to_string()),
        "backquote" | "`" => return Some("Backquote".to_string()),
        "minus" | "-" => return Some("Minus".to_string()),
        "equal" | "=" => return Some("Equal".to_string()),
        "bracketleft" | "[" => return Some("BracketLeft".to_string()),
        "bracketright" | "]" => return Some("BracketRight".to_string()),
        "backslash" | "\\" => return Some("Backslash".to_string()),
        "semicolon" | ";" => return Some("Semicolon".to_string()),
        "quote" | "'" => return Some("Quote".to_string()),
        "comma" | "," => return Some("Comma".to_string()),
        "period" | "." => return Some("Period".to_string()),
        "slash" | "/" => return Some("Slash".to_string()),
        _ => {}
    }

    if let Some(rest) = lower.strip_prefix('f') {
        if let Ok(num) = rest.parse::<u8>() {
            if (1..=24).contains(&num) {
                return Some(format!("F{num}"));
            }
        }
    }

    if trimmed.len() == 1 {
        let ch = trimmed.chars().next()?;
        if ch.is_ascii_alphanumeric() {
            return Some(ch.to_ascii_uppercase().to_string());
        }
    }

    None
}

pub fn normalize_keybinding(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut main_key: Option<String> = None;

    for part in trimmed.split('+') {
        let token = part.trim();
        if token.is_empty() {
            return None;
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            _ => {
                if main_key.is_some() {
                    return None;
                }
                main_key = Some(normalize_key_token(token)?);
            }
        }
    }

    let key = main_key?;
    let mut parts = Vec::with_capacity(4);
    if ctrl {
        parts.push("Ctrl".to_string());
    }
    if alt {
        parts.push("Alt".to_string());
    }
    if shift {
        parts.push("Shift".to_string());
    }
    parts.push(key);
    Some(parts.join("+"))
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

impl Keybindings {
    pub fn get(&self, action: KeybindingAction) -> &str {
        match action {
            KeybindingAction::ToggleSettingsOverlay => &self.toggle_settings_overlay,
            KeybindingAction::ToggleFpsHud => &self.toggle_fps_hud,
            KeybindingAction::ReloadGameView => &self.reload_game_view,
            KeybindingAction::HideToTray => &self.hide_to_tray,
            KeybindingAction::ToggleAudioMute => &self.toggle_audio_mute,
            KeybindingAction::ToggleFullscreen => &self.toggle_fullscreen,
        }
    }

    pub fn set(&mut self, action: KeybindingAction, shortcut: String) {
        match action {
            KeybindingAction::ToggleSettingsOverlay => self.toggle_settings_overlay = shortcut,
            KeybindingAction::ToggleFpsHud => self.toggle_fps_hud = shortcut,
            KeybindingAction::ReloadGameView => self.reload_game_view = shortcut,
            KeybindingAction::HideToTray => self.hide_to_tray = shortcut,
            KeybindingAction::ToggleAudioMute => self.toggle_audio_mute = shortcut,
            KeybindingAction::ToggleFullscreen => self.toggle_fullscreen = shortcut,
        }
    }

    pub fn action_for_shortcut(&self, shortcut: &str) -> Option<KeybindingAction> {
        let normalized = normalize_keybinding(shortcut)?;
        KeybindingAction::ALL.into_iter().find(|&action| {
            normalize_keybinding(self.get(action)).as_deref() == Some(normalized.as_str())
        })
    }

    pub fn validate_and_normalize(&self) -> Result<Self, KeybindingError> {
        let mut normalized = Self::default();
        let mut seen: Vec<(KeybindingAction, String)> = Vec::with_capacity(6);
        for action in KeybindingAction::ALL {
            let raw = self.get(action);
            let Some(clean) = normalize_keybinding(raw) else {
                return Err(KeybindingError::InvalidShortcut(raw.to_string()));
            };
            if let Some(&(existing_action, _)) = seen.iter().find(|(_, s)| s == &clean) {
                return Err(KeybindingError::DuplicateShortcut {
                    shortcut: clean,
                    existing_action,
                });
            }
            seen.push((action, clean.clone()));
            normalized.set(action, clean);
        }
        Ok(normalized)
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
            window_visible: true,
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

    pub fn update_settings(&mut self, mut settings: ClientSettings) -> io::Result<()> {
        if let Ok(normalized_kb) = settings.keybindings.validate_and_normalize() {
            settings.keybindings = normalized_kb;
        } else {
            settings.keybindings = self.settings.keybindings.clone();
        }
        self.settings = settings;
        self.save_settings()
    }

    pub fn update_keybinding(
        &mut self,
        action: KeybindingAction,
        shortcut: &str,
    ) -> Result<Vec<PlatformEffect>, KeybindingError> {
        let Some(normalized) = normalize_keybinding(shortcut) else {
            return Err(KeybindingError::InvalidShortcut(shortcut.to_string()));
        };

        if let Some(existing_action) = self.settings.keybindings.action_for_shortcut(&normalized) {
            if existing_action != action {
                return Err(KeybindingError::DuplicateShortcut {
                    shortcut: normalized,
                    existing_action,
                });
            }
        }

        self.settings.keybindings.set(action, normalized);
        let _ = self.save_settings();
        Ok(vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )])
    }

    pub fn restore_default_keybindings(&mut self) -> Vec<PlatformEffect> {
        self.settings.keybindings = Keybindings::default();
        let _ = self.save_settings();
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
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

    pub fn is_settings_overlay_open(&self) -> bool {
        self.settings_overlay_open
    }

    pub fn set_settings_overlay_open(&mut self, open: bool) -> Vec<PlatformEffect> {
        self.settings_overlay_open = open;
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn toggle_settings_overlay(&mut self) -> Vec<PlatformEffect> {
        self.set_settings_overlay_open(!self.settings_overlay_open)
    }

    pub fn toggle_fps_hud(&mut self) -> Vec<PlatformEffect> {
        self.settings.fps_hud_visible = !self.settings.fps_hud_visible;
        let _ = self.save_settings();
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn toggle_fullscreen(&mut self) -> Vec<PlatformEffect> {
        self.settings.fullscreen = !self.settings.fullscreen;
        let _ = self.save_settings();
        vec![
            PlatformEffect::SetFullscreen(self.settings.fullscreen),
            PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
        ]
    }

    pub fn trigger_action(&mut self, action: KeybindingAction) -> Vec<PlatformEffect> {
        match action {
            KeybindingAction::ToggleSettingsOverlay => self.toggle_settings_overlay(),
            KeybindingAction::ToggleFpsHud => self.toggle_fps_hud(),
            KeybindingAction::ReloadGameView => vec![PlatformEffect::ReloadGameView],
            KeybindingAction::HideToTray => self.hide_to_tray(),
            KeybindingAction::ToggleAudioMute => self.toggle_audio_mute(),
            KeybindingAction::ToggleFullscreen => self.toggle_fullscreen(),
        }
    }

    pub fn trigger_keybinding(&mut self, shortcut: &str) -> Vec<PlatformEffect> {
        if !self.window_focused || !self.window_visible || self.window_minimized {
            return Vec::new();
        }

        let trimmed = shortcut.trim();
        if trimmed.eq_ignore_ascii_case("escape") || trimmed.eq_ignore_ascii_case("esc") {
            if self.settings_overlay_open {
                return self.set_settings_overlay_open(false);
            }
            return Vec::new();
        }

        let Some(action) = self.settings.keybindings.action_for_shortcut(trimmed) else {
            return Vec::new();
        };

        self.trigger_action(action)
    }

    pub fn effective_fps_cap(&self) -> Option<u32> {
        if self.window_focused && !self.window_minimized && self.window_visible {
            self.settings.foreground_fps_cap.as_fps()
        } else {
            self.settings.background_fps_cap.as_fps()
        }
    }

    pub fn active_fps_cap(&self) -> Option<u32> {
        self.effective_fps_cap()
    }

    pub fn tray_menu_state(&self) -> TrayMenuState {
        TrayMenuState {
            window_visible: self.window_visible,
            audio_muted: self.settings.audio_muted,
        }
    }

    pub fn webview_state(&self) -> WebViewClientState {
        let cap = self.effective_fps_cap();
        WebViewClientState {
            settings: self.settings.clone(),
            window_focused: self.window_focused,
            window_visible: self.window_visible,
            window_minimized: self.window_minimized,
            effective_fps_cap: cap,
            settings_overlay_open: self.settings_overlay_open,
            active_fps_cap: cap,
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
    let settingsOverlayElement = null;
    let fgSelectElement = null;
    let bgSelectElement = null;
    let toggleElements = {{}};
    let keybindButtons = {{}};
    let keybindStatusElement = null;
    let recordingAction = null;
    let keybindStatusText = '';
    const consumedKeys = new Set();

    const KEYBIND_ACTIONS = [
        {{ key: 'toggle_settings_overlay', label: 'Toggle Settings Overlay' }},
        {{ key: 'toggle_fps_hud', label: 'Toggle FPS HUD' }},
        {{ key: 'reload_game_view', label: 'Reload Game View' }},
        {{ key: 'hide_to_tray', label: 'Hide to System Tray' }},
        {{ key: 'toggle_audio_mute', label: 'Toggle Audio Mute' }},
        {{ key: 'toggle_fullscreen', label: 'Toggle Fullscreen' }}
    ];

    const DEFAULT_KEYBINDINGS = {{
        toggle_settings_overlay: 'F2',
        toggle_fps_hud: 'F3',
        reload_game_view: 'F5',
        hide_to_tray: 'F9',
        toggle_audio_mute: 'F10',
        toggle_fullscreen: 'F11'
    }};

    function onElementEvent(el, type, handler) {{
        if (el && typeof el.addEventListener === 'function') {{
            el.addEventListener(type, handler);
        }}
    }}

    function formatCapLabel(cap) {{
        if (cap === null || cap === undefined) {{
            return 'Unlimited';
        }}
        if (cap === 0) {{
            return '0 FPS (Paused)';
        }}
        return String(cap) + ' FPS';
    }}

    function presetToFps(preset) {{
        switch (preset) {{
            case 'Fps0': return 0;
            case 'Fps5': return 5;
            case 'Fps15': return 15;
            case 'Fps30': return 30;
            case 'Fps60': return 60;
            case 'Fps120': return 120;
            case 'Fps144': return 144;
            case 'Fps240': return 240;
            case 'Unlimited': return null;
            default: return null;
        }}
    }}

    function recomputeEffectiveCap(st) {{
        if (!st || !st.settings) return null;
        const isForeground = st.window_focused !== false && !st.window_minimized && st.window_visible !== false;
        const cap = isForeground
            ? presetToFps(st.settings.foreground_fps_cap)
            : presetToFps(st.settings.background_fps_cap);
        st.effective_fps_cap = cap;
        st.active_fps_cap = cap;
        return cap;
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

    function syncSettingsOverlay() {{
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        if (!settingsOverlayElement) {{
            return;
        }}
        const open = Boolean(st && st.settings_overlay_open);
        settingsOverlayElement.style.display = open ? 'flex' : 'none';
        settingsOverlayElement.style.pointerEvents = open ? 'auto' : 'none';
        if (!st || !st.settings) {{
            return;
        }}
        if (fgSelectElement) {{
            fgSelectElement.value = st.settings.foreground_fps_cap || 'Unlimited';
        }}
        if (bgSelectElement) {{
            bgSelectElement.value = st.settings.background_fps_cap || 'Fps15';
        }}
        if (toggleElements.fps_hud_visible) {{
            toggleElements.fps_hud_visible.checked = Boolean(st.settings.fps_hud_visible);
        }}
        if (toggleElements.keep_awake) {{
            toggleElements.keep_awake.checked = Boolean(st.settings.keep_awake);
        }}
        if (toggleElements.close_to_tray) {{
            toggleElements.close_to_tray.checked = Boolean(st.settings.close_to_tray);
        }}
        if (toggleElements.audio_muted) {{
            toggleElements.audio_muted.checked = Boolean(st.settings.audio_muted);
        }}
        if (toggleElements.fullscreen) {{
            toggleElements.fullscreen.checked = Boolean(st.settings.fullscreen);
        }}
        const kb = st.settings.keybindings || DEFAULT_KEYBINDINGS;
        for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {{
            const act = KEYBIND_ACTIONS[i].key;
            const btn = keybindButtons[act];
            if (btn) {{
                btn.textContent = (recordingAction === act)
                    ? 'Press a key...'
                    : (kb[act] || DEFAULT_KEYBINDINGS[act]);
            }}
        }}
        if (keybindStatusElement) {{
            keybindStatusElement.textContent = keybindStatusText;
        }}
    }}

    function ensureOverlayMounted() {{
        if (hudElement && settingsOverlayElement) {{
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
                'pointer-events: none; user-select: none; z-index: 2147483647; }} ' +
                '#blockidle-settings-overlay {{ position: fixed; top: 0; left: 0; width: 100%; height: 100%; ' +
                'background: rgba(8, 10, 15, 0.72); display: none; align-items: center; justify-content: center; ' +
                'font-family: system-ui, -apple-system, sans-serif; color: #e6edf3; z-index: 2147483647; pointer-events: auto; }} ' +
                '#blockidle-settings-panel {{ width: 460px; max-width: 92vw; max-height: 88vh; overflow-y: auto; ' +
                'background: #131722; border: 1px solid rgba(255, 255, 255, 0.16); border-radius: 12px; padding: 20px; ' +
                'box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6); }} ' +
                '.bi-row {{ display: flex; align-items: center; justify-content: space-between; padding: 8px 0; border-bottom: 1px solid rgba(255,255,255,0.06); }} ' +
                '.bi-btn, select {{ background: #1f2637; color: #e6edf3; border: 1px solid rgba(255,255,255,0.2); border-radius: 6px; padding: 6px 12px; cursor: pointer; font-size: 13px; }}';
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

        let overlay = shadow.getElementById ? shadow.getElementById('blockidle-settings-overlay') : null;
        if (!overlay) {{
            overlay = document.createElement('div');
            overlay.id = 'blockidle-settings-overlay';
            overlay.style.display = 'none';
            overlay.style.pointerEvents = 'none';

            const panel = document.createElement('div');
            panel.id = 'blockidle-settings-panel';

            const header = document.createElement('div');
            header.className = 'bi-row';
            const title = document.createElement('h2');
            title.textContent = 'Settings Overlay';
            const closeBtn = document.createElement('button');
            closeBtn.id = 'blockidle-close-overlay';
            closeBtn.className = 'bi-btn';
            closeBtn.textContent = 'Close (Esc)';
            onElementEvent(closeBtn, 'click', function() {{
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                if (st) {{
                    recordingAction = null;
                    st.settings_overlay_open = false;
                    syncSettingsOverlay();
                    sendIpc({{ type: 'set_settings_overlay_open', open: false }});
                }}
            }});
            header.appendChild(title);
            header.appendChild(closeBtn);
            panel.appendChild(header);

            // Foreground FPS Cap select
            const fgRow = document.createElement('div');
            fgRow.className = 'bi-row';
            const fgLabel = document.createElement('label');
            fgLabel.textContent = 'Foreground FPS Cap';
            const fgSelect = document.createElement('select');
            fgSelect.id = 'blockidle-fg-fps-select';
            const fgPresets = [
                ['Fps15', '15 FPS'],
                ['Fps30', '30 FPS'],
                ['Fps60', '60 FPS'],
                ['Fps120', '120 FPS'],
                ['Fps144', '144 FPS'],
                ['Fps240', '240 FPS'],
                ['Unlimited', 'Unlimited']
            ];
            for (let i = 0; i < fgPresets.length; i++) {{
                const opt = document.createElement('option');
                opt.value = fgPresets[i][0];
                opt.textContent = fgPresets[i][1];
                fgSelect.appendChild(opt);
            }}
            onElementEvent(fgSelect, 'change', function() {{
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                if (st && st.settings) {{
                    st.settings.foreground_fps_cap = fgSelect.value;
                    recomputeEffectiveCap(st);
                    window.__blockidleApplyState(st);
                    sendIpc({{ type: 'update_settings', settings: st.settings }});
                }}
            }});
            fgRow.appendChild(fgLabel);
            fgRow.appendChild(fgSelect);
            panel.appendChild(fgRow);
            fgSelectElement = fgSelect;

            // Background FPS Cap select
            const bgRow = document.createElement('div');
            bgRow.className = 'bi-row';
            const bgLabel = document.createElement('label');
            bgLabel.textContent = 'Background FPS Cap';
            const bgSelect = document.createElement('select');
            bgSelect.id = 'blockidle-bg-fps-select';
            const bgPresets = [
                ['Fps0', '0 FPS (Paused)'],
                ['Fps5', '5 FPS'],
                ['Fps15', '15 FPS'],
                ['Fps30', '30 FPS'],
                ['Fps60', '60 FPS']
            ];
            for (let i = 0; i < bgPresets.length; i++) {{
                const opt = document.createElement('option');
                opt.value = bgPresets[i][0];
                opt.textContent = bgPresets[i][1];
                bgSelect.appendChild(opt);
            }}
            onElementEvent(bgSelect, 'change', function() {{
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                if (st && st.settings) {{
                    st.settings.background_fps_cap = bgSelect.value;
                    recomputeEffectiveCap(st);
                    window.__blockidleApplyState(st);
                    sendIpc({{ type: 'update_settings', settings: st.settings }});
                }}
            }});
            bgRow.appendChild(bgLabel);
            bgRow.appendChild(bgSelect);
            panel.appendChild(bgRow);
            bgSelectElement = bgSelect;

            // Toggles
            const toggleDefs = [
                {{ id: 'blockidle-toggle-fps-hud', field: 'fps_hud_visible', label: 'FPS HUD' }},
                {{ id: 'blockidle-toggle-keep-awake', field: 'keep_awake', label: 'Keep Awake' }},
                {{ id: 'blockidle-toggle-close-to-tray', field: 'close_to_tray', label: 'Close to Tray' }},
                {{ id: 'blockidle-toggle-audio-mute', field: 'audio_muted', label: 'Audio Mute' }},
                {{ id: 'blockidle-toggle-fullscreen', field: 'fullscreen', label: 'Fullscreen' }}
            ];
            for (let i = 0; i < toggleDefs.length; i++) {{
                const def = toggleDefs[i];
                const row = document.createElement('div');
                row.className = 'bi-row';
                const lbl = document.createElement('label');
                lbl.textContent = def.label;
                const input = document.createElement('input');
                input.type = 'checkbox';
                input.id = def.id;
                onElementEvent(input, 'change', function() {{
                    const st = window.__BLOCKIDLE_CLIENT_STATE__;
                    if (st && st.settings) {{
                        st.settings[def.field] = Boolean(input.checked);
                        window.__blockidleApplyState(st);
                        sendIpc({{ type: 'update_settings', settings: st.settings }});
                    }}
                }});
                row.appendChild(lbl);
                row.appendChild(input);
                panel.appendChild(row);
                toggleElements[def.field] = input;
            }}

            // Keybindings section
            const kbStatus = document.createElement('div');
            kbStatus.id = 'blockidle-keybind-status';
            panel.appendChild(kbStatus);
            keybindStatusElement = kbStatus;

            for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {{
                const item = KEYBIND_ACTIONS[i];
                const row = document.createElement('div');
                row.className = 'bi-row';
                const lbl = document.createElement('span');
                lbl.textContent = item.label;
                const btn = document.createElement('button');
                btn.id = 'blockidle-keybind-' + item.key;
                btn.className = 'bi-btn';
                onElementEvent(btn, 'click', function() {{
                    recordingAction = item.key;
                    keybindStatusText = 'Press a key or Esc to cancel';
                    syncSettingsOverlay();
                }});
                row.appendChild(lbl);
                row.appendChild(btn);
                panel.appendChild(row);
                keybindButtons[item.key] = btn;
            }}

            const restoreBtn = document.createElement('button');
            restoreBtn.id = 'blockidle-restore-defaults';
            restoreBtn.className = 'bi-btn';
            restoreBtn.textContent = 'Restore Defaults';
            onElementEvent(restoreBtn, 'click', function() {{
                const st = window.__BLOCKIDLE_CLIENT_STATE__;
                recordingAction = null;
                keybindStatusText = '';
                if (st && st.settings) {{
                    st.settings.keybindings = Object.assign({{}}, DEFAULT_KEYBINDINGS);
                }}
                syncSettingsOverlay();
                sendIpc({{ type: 'restore_default_keybindings' }});
            }});
            panel.appendChild(restoreBtn);

            overlay.appendChild(panel);
            shadow.appendChild(overlay);
        }}
        settingsOverlayElement = overlay;
        updateFpsHud();
        syncSettingsOverlay();
    }}

    function normalizeEventShortcut(event) {{
        if (!event || typeof event.key !== 'string') {{
            return null;
        }}
        const rawKey = event.key;
        const lower = rawKey.toLowerCase();
        if (lower === 'control' || lower === 'ctrl' || lower === 'shift' || lower === 'alt' || lower === 'meta' || lower === 'os') {{
            return {{ isModifierOnly: true }};
        }}
        if (lower === 'escape' || lower === 'esc') {{
            return {{ isEscape: true }};
        }}

        let mainKey = null;
        if (/^f([1-9]|1[0-9]|2[0-4])$/i.test(rawKey)) {{
            mainKey = rawKey.toUpperCase();
        }} else if (rawKey.length === 1 && /^[a-z0-9]$/i.test(rawKey)) {{
            mainKey = rawKey.toUpperCase();
        }} else {{
            const map = {{
                'enter': 'Enter',
                'return': 'Enter',
                ' ': 'Space',
                'space': 'Space',
                'spacebar': 'Space',
                'tab': 'Tab',
                'backspace': 'Backspace',
                'delete': 'Delete',
                'del': 'Delete',
                'insert': 'Insert',
                'ins': 'Insert',
                'home': 'Home',
                'end': 'End',
                'pageup': 'PageUp',
                'pagedown': 'PageDown',
                'arrowup': 'ArrowUp',
                'arrowdown': 'ArrowDown',
                'arrowleft': 'ArrowLeft',
                'arrowright': 'ArrowRight',
                '`': 'Backquote',
                '-': 'Minus',
                '=': 'Equal',
                '[': 'BracketLeft',
                ']': 'BracketRight',
                '\\': 'Backslash',
                ';': 'Semicolon',
                '\'': 'Quote',
                ',': 'Comma',
                '.': 'Period',
                '/': 'Slash'
            }};
            if (map[lower]) {{
                mainKey = map[lower];
            }}
        }}

        if (!mainKey) {{
            return null;
        }}

        const parts = [];
        if (event.ctrlKey) parts.push('Ctrl');
        if (event.altKey) parts.push('Alt');
        if (event.shiftKey) parts.push('Shift');
        parts.push(mainKey);
        return {{ shortcut: parts.join('+') }};
    }}

    function consumeKeyboardEvent(event) {{
        if (!event) return;
        if (typeof event.preventDefault === 'function') event.preventDefault();
        if (typeof event.stopImmediatePropagation === 'function') event.stopImmediatePropagation();
        if (typeof event.stopPropagation === 'function') event.stopPropagation();
    }}

    function findActionForShortcut(kb, shortcut) {{
        if (!kb || !shortcut) return null;
        const target = shortcut.toLowerCase();
        for (let i = 0; i < KEYBIND_ACTIONS.length; i++) {{
            const act = KEYBIND_ACTIONS[i].key;
            const bound = kb[act];
            if (typeof bound === 'string' && bound.toLowerCase() === target) {{
                return act;
            }}
        }}
        return null;
    }}

    function handleKeyDownCapture(event) {{
        if (!event || event.__blockidleHandled) {{
            return;
        }}
        event.__blockidleHandled = true;

        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const keyId = event.code || event.key || '';

        if (recordingAction !== null) {{
            consumeKeyboardEvent(event);
            consumedKeys.add(keyId);
            const parsed = normalizeEventShortcut(event);
            if (!parsed) {{
                return;
            }}
            if (parsed.isEscape) {{
                recordingAction = null;
                keybindStatusText = '';
                syncSettingsOverlay();
                return;
            }}
            if (parsed.isModifierOnly) {{
                return;
            }}
            if (parsed.shortcut && st && st.settings && st.settings.keybindings) {{
                const existing = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
                if (existing && existing !== recordingAction) {{
                    recordingAction = null;
                    keybindStatusText = 'Conflict: ' + parsed.shortcut + ' is already bound';
                    syncSettingsOverlay();
                    return;
                }}
                const action = recordingAction;
                recordingAction = null;
                keybindStatusText = '';
                st.settings.keybindings[action] = parsed.shortcut;
                syncSettingsOverlay();
                sendIpc({{ type: 'update_keybinding', action: action, shortcut: parsed.shortcut }});
            }}
            return;
        }}

        if (st && st.settings_overlay_open) {{
            consumeKeyboardEvent(event);
            consumedKeys.add(keyId);
            const parsed = normalizeEventShortcut(event);
            if (!parsed) {{
                return;
            }}
            if (parsed.isEscape) {{
                st.settings_overlay_open = false;
                syncSettingsOverlay();
                sendIpc({{ type: 'set_settings_overlay_open', open: false }});
                return;
            }}
            if (parsed.shortcut && st.settings && st.settings.keybindings) {{
                const matched = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
                if (matched === 'toggle_settings_overlay') {{
                    st.settings_overlay_open = false;
                    syncSettingsOverlay();
                    sendIpc({{ type: 'trigger_keybinding', shortcut: parsed.shortcut }});
                }} else if (matched) {{
                    sendIpc({{ type: 'trigger_keybinding', shortcut: parsed.shortcut }});
                }}
            }}
            return;
        }}

        if (!st || st.window_focused === false || st.window_visible === false || st.window_minimized) {{
            return;
        }}

        const parsed = normalizeEventShortcut(event);
        if (!parsed || !parsed.shortcut || !st.settings || !st.settings.keybindings) {{
            return;
        }}

        const matchedAction = findActionForShortcut(st.settings.keybindings, parsed.shortcut);
        if (!matchedAction) {{
            return;
        }}

        consumeKeyboardEvent(event);
        consumedKeys.add(keyId);
        ensureOverlayMounted();

        if (matchedAction === 'toggle_settings_overlay') {{
            st.settings_overlay_open = !st.settings_overlay_open;
            syncSettingsOverlay();
        }} else if (matchedAction === 'toggle_fps_hud') {{
            st.settings.fps_hud_visible = !st.settings.fps_hud_visible;
            updateFpsHud();
            syncSettingsOverlay();
        }} else if (matchedAction === 'toggle_audio_mute') {{
            st.settings.audio_muted = !st.settings.audio_muted;
            syncAudioMuteState();
            syncSettingsOverlay();
        }} else if (matchedAction === 'toggle_fullscreen') {{
            st.settings.fullscreen = !st.settings.fullscreen;
            syncSettingsOverlay();
        }}

        sendIpc({{ type: 'trigger_keybinding', shortcut: parsed.shortcut }});
    }}

    function handleKeyUpCapture(event) {{
        if (!event || event.__blockidleHandled) {{
            return;
        }}
        event.__blockidleHandled = true;
        const st = window.__BLOCKIDLE_CLIENT_STATE__;
        const keyId = event.code || event.key || '';
        if (recordingAction !== null || (st && st.settings_overlay_open) || consumedKeys.has(keyId)) {{
            consumedKeys.delete(keyId);
            consumeKeyboardEvent(event);
        }}
    }}

    window.addEventListener('keydown', handleKeyDownCapture, true);
    document.addEventListener('keydown', handleKeyDownCapture, true);
    window.addEventListener('keyup', handleKeyUpCapture, true);
    document.addEventListener('keyup', handleKeyUpCapture, true);

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
        const prevCap = getEffectiveCap();
        window.__BLOCKIDLE_CLIENT_STATE__ = nextState;
        syncAudioMuteState();
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
        syncSettingsOverlay();
        window.dispatchEvent(new CustomEvent('blockidle:state', {{ detail: nextState }}));
    }};

    syncAudioMuteState();

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
            syncAudioMuteState();
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
            IpcMessage::UpdateKeybinding { action, shortcut } => {
                match self.update_keybinding(action, &shortcut) {
                    Ok(effects) => effects,
                    Err(_) => vec![PlatformEffect::SyncWebViewState(
                        self.webview_state_sync_script(),
                    )],
                }
            }
            IpcMessage::RestoreDefaultKeybindings => self.restore_default_keybindings(),
            IpcMessage::TriggerKeybinding { shortcut } => self.trigger_keybinding(&shortcut),
            IpcMessage::ToggleSettingsOverlay => self.toggle_settings_overlay(),
            IpcMessage::SetSettingsOverlayOpen { open } => self.set_settings_overlay_open(open),
            IpcMessage::ToggleFpsHud => self.toggle_fps_hud(),
            IpcMessage::OpenExternalUrl { url } => {
                let (_, effects) = self.handle_new_window_request(&url);
                effects
            }
            IpcMessage::ReloadGameView => vec![PlatformEffect::ReloadGameView],
            IpcMessage::ToggleFullscreen => self.toggle_fullscreen(),
            IpcMessage::ToggleAudioMute => self.toggle_audio_mute(),
            IpcMessage::HideToTray => self.hide_to_tray(),
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
