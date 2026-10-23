# Research Report: In-App Auto-Update Architecture for Native Rust Desktop Applications (Windows & Linux)

**Target Artifact Path:** `docs/research/auto-update-mechanisms.md`  
**Date:** October 2026  
**Audience:** Architecture and Core Engineering  

---

## Executive Summary

This investigation evaluates the architecture, OS-level file locking behaviors, security boundaries, and industry best practices for implementing direct in-app auto-updates in a native Rust desktop application (Blockidle) targeting **Windows (x86_64)** and **Linux (x86_64 AppImage and standalone `tar.gz`)**.

Based on verification against primary sources—including the Linux kernel virtual filesystem (VFS) semantics, Microsoft Win32 loader and NTFS file sharing specifications, official AppImage runtime specifications, and the source code of both the `self-replace` crate and Tauri's official updater (`tauri-plugin-updater`):

1. **Proposed Architecture Confirmed Optimal:** Direct in-app auto-update via GitHub Releases API, in-app staging, and atomic binary replacement (using `self_replace` for Windows `.exe` & standalone Linux binaries, and `$APPIMAGE` atomic replacement for Linux AppImages) is **technically sound, safe, and superior** to external batch or PowerShell helper scripts, which frequently trigger antivirus heuristics and execution policy blocks.
2. **Key Nuance in Tauri's Windows Updater:** Tauri's `tauri-plugin-updater` **does not** use `self_replace` on Windows. Instead, it relies on launching an external **NSIS or MSI installer** via Win32 `ShellExecuteW` and immediately terminating the running app (`std::process::exit(0)`). For single-executable portable desktop apps, however, `self-replace` provides an in-place update without requiring an MSI/NSIS installer framework.
3. **AppImage Replacement Safety:** Replacing an active AppImage on Linux via atomic `rename(2)` is **fully safe, POSIX-standard, and leak-free**. While direct in-place writes to an active ELF executable return `ETXTBSY` ("Text file busy"), atomic `rename(2)` replaces the directory entry (`dentry`) while the active FUSE daemon and running processes seamlessly maintain their open file descriptor references to the unlinked inode until shutdown.
4. **Critical Safeguards Required:** (a) Downloads must be staged in the **same filesystem volume / parent directory** as the target to prevent `EXDEV` ("Invalid cross-device link") errors; (b) Linux AppImages must resolve the target file from `$APPIMAGE`, not `std::env::current_exe()`; (c) SHA-256 or Minisign verification must precede any filesystem mutation; and (d) Permission boundaries must be respected (user-writable directories vs. admin-owned system directories).

---

## 1. Windows Binary Replacement & `self_replace`

### 1.1 How Windows Handles Running Executables

When a PE/COFF executable (`.exe`) is launched on Windows via `CreateProcessW`:
1. **Memory Manager Section Mapping (`SEC_IMAGE`):** The Windows NT kernel loader opens the executable file and creates an image section object backed by the physical file on disk via `MmCreateSection`. Pages of executable code are paged into virtual memory on-demand from the file rather than copied wholesale into RAM.
2. **File Sharing Mode:** The loader opens the executable handle specifying `FILE_SHARE_READ | FILE_SHARE_DELETE`, but **omits** `FILE_SHARE_WRITE`.
3. **Direct Write / Truncate Rejection:** Any attempt by another process (or the process itself) to open the file with write permissions (`GENERIC_WRITE`, `FILE_WRITE_DATA`, or `TRUNCATE_EXISTING`) is rejected by the I/O manager with `ERROR_SHARING_VIOLATION` (Win32 error code 32).
4. **Direct Deletion Rejection:** Calling `DeleteFileW` on a running executable fails with `ERROR_ACCESS_DENIED` (Win32 error code 5) because NTFS explicitly prevents the deletion/unlinking of files that back active memory-mapped sections.
5. **The Renaming Loophole / Capability:** Because the loader opened the file with `FILE_SHARE_DELETE`, **renaming the running file is explicitly permitted by NTFS**. In Windows NT, renaming a file (via `MoveFileExW` or `SetFileInformationByHandle` with `FileRenameInformation`) requires `DELETE` access on the directory link. Since `FILE_SHARE_DELETE` was granted at process launch:
   ```c
   MoveFileExW(L"app.exe", L"app.exe.old", MOVEFILE_REPLACE_EXISTING);
   ```
   **This operation succeeds even while `app.exe` is actively running.**
6. **Slot Vacancy:** Once `app.exe` has been renamed to `app.exe.old`, the directory entry `app.exe` becomes available. A newly downloaded executable can immediately be written, copied, or renamed into `app.exe`.
7. **Cleanup:** When the original process terminates, the image section mapping is unmapped, all file handles are closed, and `app.exe.old` can safely be unlinked.

*Primary Source Citations:*
* Microsoft Learn: [`CreateFileW` sharing mode semantics (`FILE_SHARE_DELETE`)](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
* Microsoft Learn: [`MoveFileExW` function reference](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
* Raymond Chen, Microsoft: ["Renaming a file is a multi-step process"](https://devblogs.microsoft.com/oldnewthing/20211022-00/?p=105822)

---

### 1.2 Under the Hood of the `self-replace` Crate

The `self-replace` crate (maintained by Armin Ronacher / `mitsuhiko/self-replace`, with over 14M downloads) encapsulates this exact mechanism in Rust:

#### On Windows (`src/windows.rs`):
1. **Renaming the Executable:**
   ```rust
   let exe = env::current_exe()?.canonicalize()?;
   let old_exe = get_temp_executable_name(get_directory_of(&exe)?, ".__relocated__.exe");
   fs::rename(&exe, &old_exe)?;
   ```
   The running binary is moved to a temporary sibling file (`.<binary>.<random>.__relocated__.exe`).
2. **Placing the New Binary:**
   ```rust
   let temp_exe = get_temp_executable_name(get_directory_of(&exe)?, ".__temp__.exe");
   fs::copy(new_executable, &temp_exe)?;
   fs::rename(&temp_exe, &exe)?;
   ```
   The new binary is copied to a temp file and atomically moved into the primary executable path.
3. **The Self-Deletion Trick for the Old Executable:**
   To clean up `.__relocated__.exe` without leaving orphaned files, `self-replace` uses a novel approach:
   - It makes a copy of the old executable named `.<binary>.<random>.__selfdelete__.exe`.
   - It opens this copy with `CreateFileW` using `FILE_FLAG_DELETE_ON_CLOSE` and `FILE_SHARE_READ | FILE_SHARE_DELETE`.
   - It duplicates the parent process handle via `DuplicateHandle(GetCurrentProcess(), ...)`.
   - It spawns `.__selfdelete__.exe` with the parent process handle and path to `.__relocated__.exe`.
   - Inside `.__selfdelete__.exe`, a CRT startup hook placed in `.CRT$XCV` (`self_delete_on_init`) intercepts execution **before `main` runs**:
     - It inspects the process name; if it ends in `.__selfdelete__.exe`, it parses the parent handle argument.
     - It calls `WaitForSingleObject(parent_process_handle, INFINITE)`, blocking until the parent process exits.
     - Once the parent terminates and unlocks `old_exe`, it calls `DeleteFileW(old_exe)`.
     - It spawns a brief `cmd.exe /c exit` process to inherit and hold the open handle, ensuring that upon `ExitProcess(0)`, the OS automatically deletes `.__selfdelete__.exe` itself via `FILE_FLAG_DELETE_ON_CLOSE`.

*Primary Source Citation:*
* GitHub: [`mitsuhiko/self-replace` source code](https://github.com/mitsuhiko/self-replace/blob/main/src/windows.rs)

---

### 1.3 How Tauri Official Updater (`tauri-plugin-updater`) Implements Windows Updates

Inspection of `tauri-apps/plugins-workspace/plugins/updater/src/updater.rs` reveals how Tauri handles Windows updates:

```rust
// tauri-plugin-updater/src/updater.rs (lines 1009-1049)
fn install_inner(&self, bytes: &[u8]) -> Result<()> {
    use windows_sys::{
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOW},
        w,
    };

    let updater_type = self.extract(bytes)?;

    if let Some(on_before_exit) = self.context.on_before_exit.as_ref() {
        on_before_exit();
    }

    let file = match &updater_type {
        WindowsUpdaterType::Nsis { path, .. } => path.as_os_str().to_os_string(),
        WindowsUpdaterType::Msi { .. } => ... // msiexec.exe
    };
    let parameters = self.updater_parameters(&updater_type);

    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            w!("open"),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_SHOW,
        )
    };
    if result as isize <= 32 {
        return Err(crate::Error::Io(...));
    }

    std::process::exit(0);
}
```

#### Key Findings on Tauri:
* **Does Tauri use `self_replace` on Windows? NO.**
* Tauri strictly targets packaged installer formats: **NSIS installer (`.exe`)** or **MSI package (`.msi`)**, optionally bundled within `.zip`.
* Tauri extracts the installer to a temporary folder, invokes Win32 `ShellExecuteW` with `open` (passing arguments like `/UPDATE`, `/passive`, and relaunch instructions `/ARGS`), and immediately calls `std::process::exit(0)`.
* The spawned NSIS/MSI installer runs out-of-process, waits for the old application PID to close, overwrites application files in `C:\Program Files\` (triggering UAC if required), and relaunches the app.
* **Why Tauri does this:** Tauri applications frequently ship as full installer packages containing Webview2 loaders, auxiliary DLLs, and deep asset directory structures in protected directories (`C:\Program Files`), where in-app unprivileged binary replacement is impossible without UAC elevation.
* **Relevance to Blockidle:** Because Blockidle is a standalone native Rust executable, an external NSIS/MSI installer is unnecessary overhead unless a traditional installer framework is desired. For portable or per-user desktop apps, `self_replace` is substantially leaner.

*Primary Source Citation:*
* GitHub: [`tauri-apps/plugins-workspace/plugins/updater/src/updater.rs`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L1009-L1050)

---

## 2. Linux AppImage In-Place Update Mechanics

### 2.1 The Official AppImage Specification and `$APPIMAGE`

According to the official AppImage runtime specification and documentation:

1. **Environment Variables Exported by the Runtime:**
   When an AppImage launches, its runtime header sets three primary environment variables:
   * **`$APPIMAGE`**: The absolute path to the `.AppImage` file on the host filesystem that the user invoked (symlinks resolved). Example: `/home/user/Applications/Blockidle.AppImage`.
   * **`$APPDIR`**: The temporary mountpoint where the runtime mounted the embedded SquashFS payload using FUSE. Example: `/tmp/.mount_BlockiXXXXXX`.
   * **`$OWD`**: The original working directory from which the command was invoked.
2. **Official Specification on `$APPIMAGE` Usage:**
   The AppImage documentation explicitly states:
   > *"APPIMAGE: Contains the absolute path to the AppImage file itself... Use case: Should be used whenever the full path to the AppImage file is required (e.g., if you need to touch, update, or read meta-information from the AppImage file itself)."*
3. **The `std::env::current_exe()` Trap:**
   Inside an AppImage, calling standard library `std::env::current_exe()` returns `/tmp/.mount_BlockiXXXXXX/usr/bin/blockidle`—a virtual file inside a read-only FUSE mount!
   **Any updater attempting to update or replace `std::env::current_exe()` directly will fail immediately with `EROFS` (Read-only file system).** An AppImage updater **must** read `$APPIMAGE`.
4. **Tauri's Integration:**
   In `tauri::Env`, Tauri defines:
   ```rust
   pub struct Env {
       pub appimage: Option<OsString>, // Wrapped from $APPIMAGE on Linux
       pub appdir: Option<OsString>,
       ...
   }
   ```
   In `tauri-plugin-updater/src/lib.rs` (lines 114–117):
   ```rust
   if let Some(appimage) = env.appimage {
       builder = builder.executable_path(appimage);
   }
   ```

*Primary Source Citations:*
* AppImage Documentation: [AppImage Environment Variables](https://docs.appimage.org/packaging-guide/environment-variables.html)
* AppImage Spec Repository: [AppImage/AppImageSpec on GitHub](https://github.com/AppImage/AppImageSpec)
* Tauri Docs: [`tauri::Env::appimage`](https://docs.rs/tauri/latest/tauri/struct.Env.html#structfield.appimage)

---

### 2.2 FUSE Mount vs. In-Place File Replacement

How does replacing an active AppImage work at the Linux kernel level?

#### Architecture of an Executing AppImage:
1. The user launches `Blockidle.AppImage`.
2. The AppImage ELF runtime executes and invokes FUSE (`squashfuse`/`libfuse`) to mount the SquashFS filesystem payload to `/tmp/.mount_XXXXXX`.
3. The FUSE daemon process maintains an **open read-only file descriptor** to the `Blockidle.AppImage` file to read compressed SquashFS data blocks on demand as the application executes.

#### What Happens on Direct Overwrite (`open` + `write` / `truncate`):
* In Unix/Linux, when an ELF binary is executing, the kernel's `execve` calls `deny_write_access(file)`, decrementing `inode->i_writecount`.
* Attempting to open the file with `O_WRONLY` or `O_TRUNC` immediately fails with **`ETXTBSY` (errno 26: "Text file busy")**.
* Furthermore, if an updater truncated or mutated the file in place, any subsequent FUSE read would fetch corrupted SquashFS blocks, causing an immediate crash (`SIGSEGV` or `SIGBUS`).

#### What Happens on Atomic Replacement (`rename(2)`):
* When replacing via POSIX `rename(2)` (`std::fs::rename` in Rust):
  1. The new AppImage is downloaded and staged in a temporary file (e.g. `Blockidle.AppImage.new`) in the **same directory** as the target.
  2. Executable permissions are set: `chmod 0755`.
  3. `std::fs::rename("Blockidle.AppImage.new", &appimage_path)` is called.
* **Kernel Inode Semantics:**
  - `rename(2)` is an operation on directory entries (`dentries`), not on the underlying file data or inode.
  - The kernel unlinks the old file's directory entry and links the directory entry to the new file's inode.
  - **The active AppImage runtime and FUSE daemon continue reading uninterrupted from the old unlinked inode**, which remains valid on disk as long as open file descriptors exist.
  - The new AppImage exists at `$APPIMAGE` under its new inode, ready for the next execution.
  - When the application shuts down, the FUSE mount unmounts, all open file descriptors to the old inode are closed, and the filesystem frees the old inode's disk space automatically.
* **Verdict:** Atomic in-place replacement of `$APPIMAGE` via `rename(2)` is **100% safe, standard, crash-proof, and leak-free**.

#### Tauri's AppImage Implementation:
In `tauri-plugin-updater/src/updater.rs` (lines 1221–1318), Tauri installs AppImages by:
1. Verifying the target path is `$APPIMAGE`.
2. Verifying the downloaded payload is a valid ELF executable.
3. Creating a backup via `std::fs::rename(&self.extract_path, tmp_app_image)` on the same filesystem.
4. Writing the new AppImage to `self.extract_path` and setting permissions `0o755`.
5. Restoring the backup if extraction/writing fails.

---

## 3. Architecture Comparison: In-App `self_replace` vs. External Helpers

| Criteria | In-App `self_replace` / Atomic `rename` | External Helper Script (`.bat` / `.ps1` / `sh`) | External Helper Process (Dedicated Binary / Installer) |
| :--- | :--- | :--- | :--- |
| **Complexity** | **Low:** Single codebase, no multi-process coordination scripts. | **Medium:** Requires shell scripting, string escaping, and timeout loops. | **High:** Requires maintaining, compiling, and bundling a secondary helper binary. |
| **Antivirus / Defender Heuristics** | **Clean:** Standard Win32 API calls (`MoveFileExW`, `DuplicateHandle`). | **High Risk:** Batch files running `ping 127.0.0.1 -n 3` in `%TEMP%` and PowerShell commands frequently trigger AV/EDR flags. | **Clean:** Standard binary, but helper must also be code-signed. |
| **PowerShell Execution Policies** | **N/A** (not used). | **High Risk:** Fails on default Windows clients where `ExecutionPolicy` is `Restricted`. | **N/A** (not used). |
| **Atomicity** | **Atomic:** Uses filesystem-level atomic rename. Either fully succeeds or leaves old binary intact. | **Non-atomic:** Script can fail midway during copy, leaving a half-written binary. | **Depends on implementation** (usually non-atomic file copies). |
| **UAC Elevation (`C:\Program Files`)** | **Cannot elevate:** Fails if the user lacks write permission to the directory. | **Can elevate:** Can trigger UAC prompt via `Start-Process -Verb RunAs`. | **Can elevate:** Can invoke `ShellExecuteW(..., "runas", ...)` for UAC elevation. |
| **Per-User / Portable / AppImage** | **Ideal:** Fully native, instant, zero external dependencies. | **Suboptimal & brittle.** | **Overkill.** |

### 3.1 Failure Modes and Permissions
1. **The Permission Boundary:**
   - **User-writable directories:** `%LOCALAPPDATA%\Programs\<App>`, portable directories, `~/Applications/`, `~/.local/bin/`. In-app `self_replace` and AppImage atomic replacement work seamlessly without elevated permissions.
   - **System directories:** `C:\Program Files` on Windows, `/usr/bin` on Linux. Any in-app replacement attempted by an unprivileged process will fail with `EACCES` / `ERROR_ACCESS_DENIED`.
   - **Recommendation:** Desktop applications that self-update in place should install to per-user directories (`%LOCALAPPDATA%` on Windows, `~/.local` or AppImage on Linux) or distribute portable builds.
2. **Restart Mechanics and Race Conditions:**
   - When updating and immediately restarting on Windows, the new process must not conflict with lingering resources from the old process (such as single-instance named pipes, locked SQLite databases, or local listening ports).
   - The new process should either:
     - Wait briefly for the parent process PID to terminate, or
     - The parent process should close its network sockets, database handles, and mutexes before calling `Command::new(...).spawn()` and `std::process::exit(0)`.

---

## 4. Concrete Recommendation for Blockidle

### 4.1 Evaluation of Proposed Design
The proposed design:
> *GitHub Releases API $\rightarrow$ in-app download $\rightarrow$ `self_replace` for Windows `.exe` & standalone Linux binary $\rightarrow$ `$APPIMAGE` replacement for AppImage $\rightarrow$ spawn new process and exit*

**Confirmation:** This architecture is **confirmed optimal, lean, and industry-standard** for Blockidle. It avoids the brittleness of shell scripts, eliminates dependencies on third-party runtime frameworks, and aligns with the production-tested patterns of `rustup` and `tauri-plugin-updater`.

---

### 4.2 Mandatory Adjustments and Safeguards to Implement

To ensure 100% reliability across edge cases, incorporate the following specific safeguards:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        AUTO-UPDATE PIPELINE                           │
└────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                 1. Check GitHub Releases (API / latest.json)
                                    │
                                    ▼
                 2. Download Update to Temporary File
                    * MUST stage in SAME DIRECTORY as target (prevents EXDEV)
                    * Naming: .<target_name>.<uuid>.tmp
                                    │
                                    ▼
                 3. Cryptographic & Hash Integrity Verification
                    * Verify SHA-256 matches release manifest
                    * (Optional/Recommended) Verify Ed25519 / Minisign
                                    │
                                    ▼
                 4. Platform-Specific Target Resolution
                    ├─ Linux AppImage: Read std::env::var("APPIMAGE")
                    ├─ Linux Standalone: std::env::current_exe()?
                    └─ Windows .exe: std::env::current_exe()?
                                    │
                                    ▼
                 5. Atomic Replacement
                    ├─ Linux (AppImage & Standalone):
                    │   • chmod 0755 on temp file
                    │   • std::fs::rename(temp_file, target_path)
                    └─ Windows (.exe):
                        • self_replace::self_replace(temp_file)
                                    │
                                    ▼
                 6. Clean Shutdown & Process Relaunch
                    • Release single-instance mutex / database locks
                    • std::process::Command::new(target_path).spawn()
                    • std::process::exit(0)
```

#### Detailed Safeguard Specifications:

1. **Guard Against Cross-Device Links (`EXDEV` / `ErrorKind::CrossesDevices`):**
   * POSIX `rename(2)` and Windows `MoveFileExW` cannot move files across different filesystem mount points.
   * If the download is staged in `/tmp` (frequently mounted as a `tmpfs` RAM disk in Linux) while `$APPIMAGE` resides in `/home` (e.g. `ext4`), `std::fs::rename` will fail with `EXDEV`!
   * **Rule:** Always create the temporary staging download file in the **same parent directory** as the destination executable (e.g. using `tempfile::Builder::new().tempfile_in(target_path.parent()?)`).
2. **AppImage Target Detection:**
   * Always check `std::env::var("APPIMAGE")` first on Linux.
   * Only fall back to `std::env::current_exe()` if `$APPIMAGE` is absent (which indicates a standalone tar.gz binary install).
3. **Preserve Linux Executable Permissions:**
   * Downloaded payloads do not automatically carry executable bits. Explicitly call:
     ```rust
     #[cfg(unix)]
     {
         use std::os::unix::fs::PermissionsExt;
         std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o755))?;
     }
     ```
     before executing the rename.
4. **Mandatory Hash / Signature Verification:**
   * Never execute binary replacement without verifying the download against a known SHA-256 hash or Minisign public key signature. A truncated download due to an interrupted connection would otherwise replace a functional app with a corrupt binary.
5. **Windows Write Permission Detection:**
   * If the application is ever placed in a non-writable directory (e.g. `C:\Program Files` without administrative privileges), catch the `PermissionDenied` error gracefully and display a prompt directing the user to download the latest installer manually from GitHub Releases, rather than failing silently or crashing.

---

## 5. Primary Source Reference Index

1. **Microsoft Win32 File Management Documentation:**
   * `CreateFileW` Sharing Modes: [learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)
   * `MoveFileExW` Specification: [learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
2. **`self-replace` Crate Source Code:**
   * GitHub Repository: [github.com/mitsuhiko/self-replace](https://github.com/mitsuhiko/self-replace)
   * Windows Implementation: [`src/windows.rs`](https://github.com/mitsuhiko/self-replace/blob/main/src/windows.rs)
   * Unix Implementation: [`src/unix.rs`](https://github.com/mitsuhiko/self-replace/blob/main/src/unix.rs)
3. **Tauri v2 Updater Plugin Source Code:**
   * Repository: [`tauri-apps/plugins-workspace`](https://github.com/tauri-apps/plugins-workspace)
   * Windows Installer Runner: [`plugins/updater/src/updater.rs#L1009-L1050`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L1009-L1050)
   * Linux AppImage Installation: [`plugins/updater/src/updater.rs#L1221-L1318`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/updater.rs#L1221-L1318)
   * Environment Variable Hook: [`plugins/updater/src/lib.rs#L114-L117`](https://github.com/tauri-apps/plugins-workspace/blob/v2/plugins/updater/src/lib.rs#L114-L117)
4. **AppImage Documentation & Specification:**
   * AppImage Environment Variables: [docs.appimage.org/packaging-guide/environment-variables.html](https://docs.appimage.org/packaging-guide/environment-variables.html)
   * AppImage Specification: [github.com/AppImage/AppImageSpec](https://github.com/AppImage/AppImageSpec)
5. **Linux Kernel VFS Semantics:**
   * `rename(2)` POSIX Specification: [man7.org/linux/man-pages/man2/rename.2.html](https://man7.org/linux/man-pages/man2/rename.2.html)
   * Kernel `deny_write_access` (`ETXTBSY`): Linux Kernel VFS `fs/exec.c` and `include/linux/fs.h`.
6. **Rustup Self-Update Architecture:**
   * Source Repository: [`rust-lang/rustup`](https://github.com/rust-lang/rustup)
   * Windows Self-Update & Wait for Parent: [`src/cli/self_update/windows.rs`](https://github.com/rust-lang/rustup/blob/master/src/cli/self_update/windows.rs#L433-L446)
