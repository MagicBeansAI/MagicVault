# Desktop platforms

The source candidate includes native backends for macOS, Linux and Windows.
Platform code, a successful build and a tested desktop flow are separate claims.
No public npm release or signed cross-platform release is announced here.

| Component | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Prompt window | Shared Rust desktop UI | Same UI, X11/Wayland | Same UI |
| Master key | macOS Keychain | Secret Service session keyring | Credential Manager |
| Local connection | Same-UID Unix socket | Same-UID Unix socket | Same-user named pipe with private DACL |
| Login-session service | LaunchAgent | systemd user unit | Per-user interactive scheduled task |
| Browser connection | CDP or Chromium native host | CDP or Chromium native host | CDP or Chromium native host |
| Saved and one-time browser fill | Implemented | Implemented | Implemented |
| HTTP delivery | Implemented | Implemented | Implemented |
| Governed process delivery | MagicRun backend | MagicRun backend | Refused as `unsupported_target` |

Windows process delivery requires a separately qualified MagicRun backend. It
does not fall back to an unrestricted subprocess. The browser credential flow
does not require that backend.

## Requirements

- **macOS:** a logged-in desktop and an accessible Keychain. Native package
  targets are `darwin-arm64` and `darwin-x64`.
- **Linux:** a graphical X11 or Wayland session, OpenGL-capable rendering, a
  running session D-Bus and a Secret Service provider such as GNOME Keyring or
  compatible KWallet. `setup` uses systemd's user manager and imports only the
  desktop display/session variables needed by the window. A headless SSH session
  or missing/locked keyring cannot silently approve or collect credentials.
  Package targets are glibc-based `linux-x64` and `linux-arm64`; musl/Alpine is
  not a prebuilt target. Without systemd, use explicit foreground `init`/`serve`.
  The prepared npm release builds use Ubuntu 22.04 / glibc 2.35; older systems
  require their own source build and qualification.
  X11 also needs `libxkbcommon-x11`; minimal Debian/Ubuntu desktops can install
  `libxkbcommon-x11-0`, `libgl1`, and `libegl1` through their package manager.
- **Windows:** a logged-in user desktop, Credential Manager, Task Scheduler and
  a local NTFS user volume. Setup creates a least-privilege interactive task;
  no elevated/system service is installed. Targets are `win32-x64` and
  `win32-arm64`, with `.exe` executables. Windows native-host registration uses
  HKCU entries for Chrome, Chromium and Edge.
  Managed task paths must not contain `%`: Task Scheduler expands environment
  variables in action paths, so setup refuses those paths. The task uses an
  existing interactive session, as specified by [Microsoft's logon-type
  contract](https://learn.microsoft.com/en-us/windows/win32/taskschd/principal-logontype).

Node launchers/SDK require Node 22+. Install matching, trusted native and launcher
tarballs, then run `magicvault --profile agent setup`. Use the exact MCP command
and extension path returned by setup; do not copy macOS paths into Windows.
Source builds also need the `magicvault-prompt` executable beside the daemon.

Default roots are `.magicvault` and `.magicvault-app` under the current user's
home (`USERPROFILE` on Windows). Unix vault roots retain the 85-byte socket-path
limit. Windows uses a hashed named-pipe name rather than a filesystem socket.
Existing keys are never regenerated as an outage workaround.

## Installation and recovery differences

Unix custody files use owner-only permissions. Windows roots/files use an
owner/SYSTEM-only DACL; unsafe files and reparse points are refused. Windows
publishes flushed file contents with a write-through replacement. It has no
unprivileged Unix directory-fsync equivalent; power-loss durability must be
qualified separately on the target filesystem.

Unix activation replaces the `current` symlink atomically. Windows uses a
directory junction, which does not need Developer Mode or administrator symlink
privileges. Replacement occurs only after draining the daemon, but includes a
short interval without `current`. An interrupted activation retains complete
old/new version directories and any staged junction; it is not an automatic
rollback. Inspect with `doctor`, then explicitly rerun setup with the matching
trusted bundle if activation is incomplete. Do not delete or reinitialize the
vault. Close browser native-host connections before replacing their Windows
executable copy; a file in use can refuse replacement.

Service/native-host definitions are user-owned and checked before replacement
or removal. Foreign or modified definitions cause a conflict. A partial setup
may require explicit reconciliation; repeating setup is not permission to adopt
an unrelated task, unit or registry entry.

## Verification

See the [2026-09-16 executed checks](qualification/results-desktop-platforms-2026-09-16.md)
for Linux test/UI results, Windows compile checks and remaining native acceptance.

The checked-in 0.8.3 demo and earlier native acceptance results cover one macOS
installation and the previous dialog renderer. They do not qualify the new UI
or Windows/Linux desktop behavior. The new platform build/test workflow and
focused tests cover the source changes. Record actual executed results in the
[qualification index](qualification/README.md) before marking desktop acceptance
complete. Windows scheduled tasks, Credential Manager, browser registration,
Linux login-session startup/keyring and headed UI behavior require native runs.
