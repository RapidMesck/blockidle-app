# Direct In-App Auto-Update via GitHub Releases API, `self_replace`, and Atomic AppImage Swapping

Distributing Blockidle client shell updates via external browser redirects or external installer/script wrappers introduces player friction and triggers antivirus execution heuristics (such as batch scripts spawned from temporary folders). Furthermore, on Linux, users run either standalone unpacked binaries or self-contained `.AppImage` packages that mount their contents via FUSE to read-only virtual paths (`/tmp/.mount_XXXXXX`).

We chose a direct, in-app auto-update architecture:

1. **Detection & Pacing:** The Client Shell queries the GitHub Releases API (`https://api.github.com/repos/RapidMesck/blockidle-app/releases/latest`) asynchronously 5 seconds after startup and on-demand via a *"Verificar Atualizações"* button in the Settings Overlay. The release tag (`vX.Y.Z`) is compared against the client's compile-time version (`env!("CARGO_PKG_VERSION")`).
2. **Non-Intrusive Notification:** Available updates are displayed in the Settings Overlay (with version notes and an *"Atualizar Agora"* button) and as a dynamic item in the System Tray menu without modal interruptions during active game sessions.
3. **On-Demand Direct In-App Download:** Clicking update triggers an asynchronous background worker that downloads the platform-matching asset with live byte-level progress reported to the UI via IPC.
4. **Platform Target Resolution & Atomic Replacement:**
   - **Windows:** Targets `blockidle-app-vX.Y.Z-windows-x86_64.exe` and applies atomic binary replacement via the `self_replace` crate (renaming the running binary to allow directory slot vacancy on NTFS).
   - **Linux (AppImage):** Targets `blockidle-app-vX.Y.Z-linux-x86_64.AppImage` when the `APPIMAGE` environment variable is present. The temporary download is staged in the **same parent directory** as `$APPIMAGE` (preventing `EXDEV` cross-device link errors), marked with `0o755` executable permissions, and atomically swapped onto `$APPIMAGE` via POSIX `rename(2)`. The active FUSE mount and processes retain their open file descriptor references to the unlinked inode until clean exit.
   - **Linux (Standalone / tar.gz):** Targets `blockidle-app-vX.Y.Z-linux-x86_64.tar.gz` when `APPIMAGE` is absent, extracts the internal binary, and replaces `std::env::current_exe()`.
5. **Integrity & Relaunch:** SHA-256 verification guards against truncated or corrupt downloads. Upon completion, the Settings Overlay presents a *"Reiniciar Agora"* button that spawns the updated executable and cleanly exits the current process.
