use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use url::Url;

pub const GAME_VIEW_INITIAL_URL: &str = "https://blockidle.io/play";

const INIT_SCRIPT_TEMPLATE: &str = include_str!("init_script.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetPlatform {
    WindowsX86_64,
    LinuxX86_64AppImage,
    LinuxX86_64Standalone,
}

impl TargetPlatform {
    pub fn current() -> Self {
        #[cfg(target_os = "windows")]
        {
            Self::WindowsX86_64
        }
        #[cfg(not(target_os = "windows"))]
        {
            if std::env::var_os("APPIMAGE").is_some() {
                Self::LinuxX86_64AppImage
            } else {
                Self::LinuxX86_64Standalone
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub tag_name: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateCheckResult {
    UpToDate,
    UpdateAvailable {
        latest_version: String,
        release_name: String,
        release_notes: String,
        release_url: String,
        asset_name: String,
        asset_url: String,
        asset_size: u64,
    },
    NoMatchingAsset {
        latest_version: String,
        release_url: String,
    },
    Error {
        message: String,
    },
}

pub fn parse_release_info(json: &str) -> Result<ReleaseInfo, String> {
    serde_json::from_str::<ReleaseInfo>(json).map_err(|e| e.to_string())
}

pub fn select_release_asset(
    assets: &[ReleaseAsset],
    target: TargetPlatform,
) -> Option<&ReleaseAsset> {
    assets.iter().find(|asset| {
        let name = &asset.name;
        match target {
            TargetPlatform::WindowsX86_64 => {
                name.ends_with(".exe") && (name.contains("windows") || name.contains("x86_64"))
            }
            TargetPlatform::LinuxX86_64AppImage => {
                name.ends_with(".AppImage") || name.ends_with(".appimage")
            }
            TargetPlatform::LinuxX86_64Standalone => {
                name.ends_with(".tar.gz") && name.contains("linux")
            }
        }
    })
}

pub fn parse_version_triplet(v: &str) -> Option<(u64, u64, u64)> {
    let clean = v.trim().strip_prefix('v').unwrap_or(v.trim());
    let mut parts = clean.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub fn compare_semver(current: &str, latest: &str) -> Option<std::cmp::Ordering> {
    let cur = parse_version_triplet(current)?;
    let lat = parse_version_triplet(latest)?;
    Some(lat.cmp(&cur))
}

pub fn evaluate_update(
    current_version: &str,
    release: &ReleaseInfo,
    target: TargetPlatform,
) -> UpdateCheckResult {
    match compare_semver(current_version, &release.tag_name) {
        Some(std::cmp::Ordering::Greater) => {
            if let Some(asset) = select_release_asset(&release.assets, target) {
                UpdateCheckResult::UpdateAvailable {
                    latest_version: release.tag_name.clone(),
                    release_name: release.name.clone(),
                    release_notes: release.body.clone(),
                    release_url: release.html_url.clone(),
                    asset_name: asset.name.clone(),
                    asset_url: asset.browser_download_url.clone(),
                    asset_size: asset.size,
                }
            } else {
                UpdateCheckResult::NoMatchingAsset {
                    latest_version: release.tag_name.clone(),
                    release_url: release.html_url.clone(),
                }
            }
        }
        _ => UpdateCheckResult::UpToDate,
    }
}

pub fn calculate_sha256(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    let mut out = String::with_capacity(64);
    for byte in digest.as_ref() {
        use std::fmt::Write;
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

#[derive(Debug)]
pub enum UpdateInstallError {
    ChecksumMismatch { expected: String, actual: String },
    Io(io::Error),
    TargetParentNotFound,
}

impl std::fmt::Display for UpdateInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChecksumMismatch { expected, actual } => {
                write!(
                    f,
                    "SHA-256 checksum mismatch: expected {expected}, got {actual}"
                )
            }
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::TargetParentNotFound => {
                write!(f, "Parent directory of target executable not found")
            }
        }
    }
}

impl std::error::Error for UpdateInstallError {}

impl From<io::Error> for UpdateInstallError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

pub fn stage_and_replace_executable(
    target_path: &Path,
    new_bytes: &[u8],
    expected_sha256: Option<&str>,
    is_appimage: bool,
) -> Result<(), UpdateInstallError> {
    if let Some(expected) = expected_sha256 {
        let actual = calculate_sha256(new_bytes);
        if !actual.eq_ignore_ascii_case(expected.trim()) {
            return Err(UpdateInstallError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
    }

    let parent = target_path
        .parent()
        .ok_or(UpdateInstallError::TargetParentNotFound)?;

    let temp_name = format!(
        ".{}.{}.tmp",
        target_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("update"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_path = parent.join(temp_name);

    fs::write(&temp_path, new_bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(&temp_path, perms);
    }

    if is_appimage {
        fs::rename(&temp_path, target_path)?;
    } else {
        #[cfg(target_os = "windows")]
        {
            self_replace::self_replace(&temp_path).map_err(io::Error::other)?;
            let _ = fs::remove_file(&temp_path);
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(current) = std::env::current_exe()
                && current == target_path
            {
                self_replace::self_replace(&temp_path).map_err(io::Error::other)?;
                let _ = fs::remove_file(&temp_path);
                return Ok(());
            }
            fs::rename(&temp_path, target_path)?;
        }
    }

    Ok(())
}

pub fn resolve_target_executable_with<F>(get_appimage_env: F) -> Result<(PathBuf, bool), String>
where
    F: FnOnce() -> Option<std::ffi::OsString>,
{
    #[cfg(target_os = "linux")]
    {
        if let Some(val) = get_appimage_env() {
            let s = val.to_string_lossy();
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Ok((PathBuf::from(trimmed), true));
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = get_appimage_env;

    let current = std::env::current_exe()
        .map_err(|e| format!("Failed to determine current executable: {e}"))?;
    Ok((current, false))
}

pub fn resolve_target_executable() -> Result<(PathBuf, bool), String> {
    resolve_target_executable_with(|| std::env::var_os("APPIMAGE"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationDecision {
    AllowInGameView,
    OpenExternalBrowser(String),
    Block,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrayMenuState {
    pub window_visible: bool,
    pub audio_muted: bool,
    #[serde(default)]
    pub update_available: Option<String>,
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
    TriggerUpdateCheck,
    TriggerUpdateDownload {
        asset_url: String,
        asset_name: String,
    },
    RelaunchApp,
    ExitApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateState {
    Idle,
    Checking,
    Available {
        latest_version: String,
        release_name: String,
        release_notes: String,
        release_url: String,
        asset_name: String,
        asset_url: String,
        asset_size: u64,
    },
    Downloading {
        downloaded: u64,
        total: u64,
    },
    ReadyToRestart,
    UpToDate,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    ClientReady,
    UpdateSettings {
        settings: ClientSettings,
    },
    UpdateKeybinding {
        action: KeybindingAction,
        shortcut: String,
    },
    RestoreDefaultKeybindings,
    TriggerKeybinding {
        shortcut: String,
    },
    ToggleSettingsOverlay,
    SetSettingsOverlayOpen {
        open: bool,
    },
    ToggleFpsHud,
    OpenExternalUrl {
        url: String,
    },
    ReloadGameView,
    ToggleFullscreen,
    ToggleAudioMute,
    HideToTray,
    CheckForUpdates,
    StartUpdate {
        asset_url: String,
        asset_name: String,
    },
    RelaunchApp,
}

fn default_app_version() -> String {
    format!("v{}", env!("CARGO_PKG_VERSION"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebViewClientState {
    pub settings: ClientSettings,
    pub window_focused: bool,
    pub window_visible: bool,
    pub window_minimized: bool,
    pub effective_fps_cap: Option<u32>,
    pub settings_overlay_open: bool,
    pub update_state: UpdateState,
    #[serde(default = "default_app_version")]
    pub app_version: String,
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

    if let Some(rest) = lower.strip_prefix('f')
        && let Ok(num) = rest.parse::<u8>()
        && (1..=24).contains(&num)
    {
        return Some(format!("F{num}"));
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
            close_to_tray: true,
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
    update_state: UpdateState,
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
            update_state: UpdateState::Idle,
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
        let config_dir = self.config_path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(config_dir)?;
        let fonts_conf_path = config_dir.join("fonts.conf");
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <include ignore_missing="yes">/etc/fonts/fonts.conf</include>
  <match target="font">
    <edit name="hinting" mode="assign"><bool>true</bool></edit>
    <edit name="hintstyle" mode="assign"><const>hintslight</const></edit>
    <edit name="autohint" mode="assign"><bool>false</bool></edit>
    <edit name="antialias" mode="assign"><bool>true</bool></edit>
    <edit name="rgba" mode="assign"><const>none</const></edit>
    <edit name="lcdfilter" mode="assign"><const>lcdnone</const></edit>
    <edit name="embeddedbitmap" mode="assign"><bool>false</bool></edit>
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

        if let Some(existing_action) = self.settings.keybindings.action_for_shortcut(&normalized)
            && existing_action != action
        {
            return Err(KeybindingError::DuplicateShortcut {
                shortcut: normalized,
                existing_action,
            });
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
        let json = serde_json::to_string_pretty(&self.settings).map_err(io::Error::other)?;
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

    pub fn update_state(&self) -> &UpdateState {
        &self.update_state
    }

    pub fn apply_update_check_result(&mut self, result: &UpdateCheckResult) -> Vec<PlatformEffect> {
        self.update_state = match result {
            UpdateCheckResult::UpdateAvailable {
                latest_version,
                release_name,
                release_notes,
                release_url,
                asset_name,
                asset_url,
                asset_size,
            } => UpdateState::Available {
                latest_version: latest_version.clone(),
                release_name: release_name.clone(),
                release_notes: release_notes.clone(),
                release_url: release_url.clone(),
                asset_name: asset_name.clone(),
                asset_url: asset_url.clone(),
                asset_size: *asset_size,
            },
            UpdateCheckResult::UpToDate => UpdateState::UpToDate,
            UpdateCheckResult::NoMatchingAsset { latest_version, .. } => UpdateState::Error {
                message: format!("No compatible release asset found for {latest_version}"),
            },
            UpdateCheckResult::Error { message } => UpdateState::Error {
                message: message.clone(),
            },
        };
        vec![
            PlatformEffect::UpdateTrayMenu(self.tray_menu_state()),
            PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
        ]
    }

    pub fn apply_update_progress(&mut self, downloaded: u64, total: u64) -> Vec<PlatformEffect> {
        self.update_state = UpdateState::Downloading { downloaded, total };
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn apply_update_ready(&mut self) -> Vec<PlatformEffect> {
        self.update_state = UpdateState::ReadyToRestart;
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn apply_update_error(&mut self, err: &str) -> Vec<PlatformEffect> {
        self.update_state = UpdateState::Error {
            message: err.to_string(),
        };
        vec![PlatformEffect::SyncWebViewState(
            self.webview_state_sync_script(),
        )]
    }

    pub fn tray_menu_state(&self) -> TrayMenuState {
        let update_available = match &self.update_state {
            UpdateState::Available { latest_version, .. } => Some(latest_version.clone()),
            _ => None,
        };
        TrayMenuState {
            window_visible: self.window_visible,
            audio_muted: self.settings.audio_muted,
            update_available,
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
            update_state: self.update_state.clone(),
            app_version: format!("v{}", env!("CARGO_PKG_VERSION")),
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
        INIT_SCRIPT_TEMPLATE
            .replacen("__BLOCKIDLE_INITIAL_STATE_JSON__", &state_json, 1)
            .replacen(
                "__BLOCKIDLE_APP_VERSION__",
                &format!("v{}", env!("CARGO_PKG_VERSION")),
                1,
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
            IpcMessage::CheckForUpdates => {
                self.update_state = UpdateState::Checking;
                vec![
                    PlatformEffect::TriggerUpdateCheck,
                    PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
                ]
            }
            IpcMessage::StartUpdate {
                asset_url,
                asset_name,
            } => {
                self.update_state = UpdateState::Downloading {
                    downloaded: 0,
                    total: 0,
                };
                vec![
                    PlatformEffect::TriggerUpdateDownload {
                        asset_url,
                        asset_name,
                    },
                    PlatformEffect::SyncWebViewState(self.webview_state_sync_script()),
                ]
            }
            IpcMessage::RelaunchApp => vec![PlatformEffect::RelaunchApp],
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
