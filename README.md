# Blockidle Desktop Client

Native desktop client shell for the [Blockidle](https://blockidle.io/play) web game, built with pure Rust using [`tao`](https://github.com/tauri-apps/tao) and [`wry`](https://github.com/tauri-apps/wry).

Provides deep system integration, frame-rate pacing, background idle execution, low-overhead performance tuning, customizable keybindings, and seamless in-app updates.

---

## Features

- **In-Page Settings Overlay (`F2`)**: Native configuration panel rendered inside an isolated Shadow DOM on top of the WebGL canvas—zero CSS conflicts, instantaneous rendering, and no external popups.
- **Configurable Frame-Rate Pacing**:
  - **Foreground FPS Cap**: `15`, `30`, `60` (default), `120`, `144`, `240`, or `Unlimited`.
  - **Background FPS Cap**: `0 FPS` (Paused), `5 FPS`, `15 FPS` (default for idle combat), `30 FPS`, or `60 FPS`.
- **Decoupled Performance Mode**:
  - Drops internal render buffer to `0.75x` on standard displays with `image-rendering: pixelated` (cuts GPU fill rate by ~44% while preserving authentic Minecraft aesthetics).
  - Explicitly disables Three.js shadow maps and directional sun shadows.
  - Throttles non-essential cosmetic particles by 80%.
- **Combat Resynchronization (`__blockidleResume`)**: Dispatches a synthetic resume event when transitioning out of background pause, triggering the game's internal `resync()` to ensure mob health bars and damage hit marks remain active.
- **Global Reload (`F5` / In-Overlay Button)**: Instantly reloads the entire game view and WebGL context from scratch with a single click or keystroke.
- **Keep Awake**: Suppresses browser visibility state degradation (`document.hidden = false`), keeping the Server-Sent Events (SSE) session alive without triggering away resets.
- **FPS HUD (`F3`)**: Lightweight real-time frame counter and cap indicator.
- **Audio Mute (`F10`)**: Client-side audio interceptor that silences the game without altering operating system audio levels.
- **Close to Tray**: Minimizes to the system tray on window close, maintaining idle progression in the background.
- **In-App Auto-Updater**: Checks GitHub Releases, downloads platform assets with progress tracking, verifies SHA-256 integrity, and performs atomic replacement on Linux (AppImage / Standalone) and Windows.

---

## Default Keybindings

| Action | Shortcut | Description |
| :--- | :--- | :--- |
| **Toggle Settings Overlay** | `F2` | Opens/closes the in-game settings panel |
| **Toggle FPS HUD** | `F3` | Displays or hides the real-time frame rate indicator |
| **Reload Game View** | `F5` | Completely reloads the game view and resets memory |
| **Toggle Audio Mute** | `F10` | Silences or restores game audio |
| **Toggle Fullscreen** | `F11` | Switches between windowed and borderless fullscreen |

*All keybindings can be customized or restored to defaults in the Settings Overlay (`F2`).*

---

## Development & Building

### Prerequisites

- **Rust**: 2024 edition (stable toolchain)
- **Linux Dependencies** (Ubuntu / Debian):
  ```bash
  sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev
  ```

### Build Commands

```bash
# Run in development mode
cargo run

# Build optimized release binary
cargo build --release

# Run comprehensive integration test suite
cargo test
```

---

## Architecture Decision Records (ADRs)

- [`0001-rust-webview-stack.md`](docs/adr/0001-rust-webview-stack.md): Pure Rust `wry` + `tao` WebView stack directly navigating to `blockidle.io/play`.
- [`0002-in-page-shadow-dom-overlay.md`](docs/adr/0002-in-page-shadow-dom-overlay.md): In-Page Shadow DOM for Settings Overlay and FPS HUD.
- [`0003-raf-and-visibility-interception.md`](docs/adr/0003-raf-and-visibility-interception.md): Frame-rate pacing via `requestAnimationFrame` and `visibilitychange` suppression.
- [`0004-direct-in-app-auto-updater.md`](docs/adr/0004-direct-in-app-auto-updater.md): Direct in-app auto-update via GitHub Releases API, `self_replace`, and atomic AppImage swapping.
- [`0005-decoupled-performance-mode-and-combat-resync.md`](docs/adr/0005-decoupled-performance-mode-and-combat-resync.md): Decoupled performance mode and synthetic combat resynchronization.

See [`GLOSSARY.md`](GLOSSARY.md) for the project domain terminology.
