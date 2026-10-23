# Blockidle Desktop Client

Native desktop wrapper and client controller for the Blockidle web game (`https://blockidle.io/play`), providing frame-rate control, shortcuts, and background idle execution.

## Language

**Client Shell**:
The native desktop application window and system tray host that manages the lifecycle, focus state, and local settings of the game session.
_Avoid_: Wrapper, browser, launcher window

**Game View**:
The embedded web view displaying the official Blockidle game session.
_Avoid_: Iframe, tab, browser page

**Settings Overlay**:
The built-in configuration panel toggled on top of the game to adjust frame-rate limits, shortcuts, and window behavior without leaving the session.
_Avoid_: Popup, external settings window, modal page

**Foreground FPS Cap**:
The maximum frame rate allowed while the Client Shell is focused and actively viewed by the player.
_Avoid_: Max FPS, active frame limit

**Background FPS Cap**:
The reduced maximum frame rate applied automatically when the Client Shell is unfocused, minimized, or hidden to the system tray (including `0 FPS` to pause 3D frame callbacks while keeping the network session alive).
_Avoid_: AFK limit, idle throttle

**FPS HUD**:
The compact on-screen indicator showing current frame rate and active frame-rate cap inside the Game View.
_Avoid_: Debug overlay, performance monitor

**Keep Awake**:
A client mode that suppresses browser tab-hiding signals (`document.hidden` / `visibilitychange`) when the window is unfocused or hidden to the tray, keeping the live SSE game session active without triggering server-side away/offline state resets.
_Avoid_: Anti-AFK, fake focus

**Close to Tray**:
A user setting that makes closing the Client Shell window hide the application to the system tray instead of terminating the process.
_Avoid_: Background exit, minimize on close

**Keybinding**:
A user-configurable keyboard shortcut intercepted by the Client Shell to trigger client actions (such as fullscreen, reload, mute, tray hide, or toggling the Settings Overlay).
_Avoid_: Hotkey, macro
