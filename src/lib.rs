use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use url::Url;

pub const GAME_VIEW_INITIAL_URL: &str = "https://blockidle.io/play";

const INIT_SCRIPT_TEMPLATE: &str = include_str!("init_script.js");

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
        Some(match self {
            Self::Fps0 => 0,
            Self::Fps5 => 5,
            Self::Fps15 => 15,
            Self::Fps30 => 30,
            Self::Fps60 => 60,
        })
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
        let mut should_persist = false;
        let settings = match fs::read_to_string(&config_path) {
            Ok(raw) => match serde_json::from_str::<ClientSettings>(&raw) {
                Ok(mut parsed) => match parsed.keybindings.validate_and_normalize() {
                    Ok(normalized_kb) => {
                        if parsed.keybindings != normalized_kb {
                            should_persist = true;
                        }
                        parsed.keybindings = normalized_kb;
                        parsed
                    }
                    Err(_) => {
                        should_persist = true;
                        ClientSettings::default()
                    }
                },
                Err(_) => {
                    should_persist = true;
                    ClientSettings::default()
                }
            },
            Err(_) => ClientSettings::default(),
        };

        let controller = Self {
            config_path,
            data_dir: data_dir.as_ref().to_path_buf(),
            settings,
            window_focused: true,
            window_visible: true,
            window_minimized: false,
            settings_overlay_open: false,
        };
        if should_persist {
            let _ = controller.save_settings();
        }
        controller
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn ensure_fontconfig_file(&self) -> io::Result<PathBuf> {
        let config_dir = self
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(config_dir)?;
        let fonts_conf_path = config_dir.join("fonts.conf");
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <match target="font">
    <edit name="hinting" mode="assign"><bool>false</bool></edit>
    <edit name="hintstyle" mode="assign"><const>hintnone</const></edit>
    <edit name="autohint" mode="assign"><bool>false</bool></edit>
    <edit name="antialias" mode="assign"><bool>true</bool></edit>
    <edit name="rgba" mode="assign"><const>none</const></edit>
  </match>
</fontconfig>
"#;
        fs::write(&fonts_conf_path, xml)?;
        Ok(fonts_conf_path)
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
            (
                NavigationDecision::OpenExternalBrowser(url.to_string()),
                vec![PlatformEffect::OpenExternalBrowser(url.to_string())],
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
        INIT_SCRIPT_TEMPLATE.replacen("__BLOCKIDLE_INITIAL_STATE_JSON__", &state_json, 1)
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
        self.window_minimized = false;
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
