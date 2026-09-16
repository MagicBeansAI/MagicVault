# Desktop platforms

The source candidate includes native backends for macOS, Linux and Windows;
**all three platforms are alpha**.
Platform code, a successful build and a tested desktop flow are separate claims.
No public npm release or signed cross-platform release is announced here.

This guide covers the standalone CLI/MCP/npm bundle. Magician's embedded
integration manages its own storage and human prompts; see
[embedded consumers](integrations.md#existing-embedded-consumers).

| Component | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Prompt window | Shared Rust desktop UI | Same UI, X11/Wayland | Same UI |
| Master key | macOS Keychain | Secret Service desktop keyring | Credential Manager |
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

## Credential storage

Saved login and card-field records use the same encrypted vault format on every
platform. MagicVault serializes the saved records and encrypts them with
**AES-256-GCM**, using a fresh random 96-bit nonce per write. The encrypted bytes
live in `vault/provisioned_secrets.vault` beneath the vault root. The OS credential
store holds the **256-bit master key** used to decrypt that file.

| Platform | Default vault root | Master-key backend |
| --- | --- | --- |
| macOS | `$HOME/.magicvault` | The user's macOS Keychain |
| Linux | `$HOME/.magicvault` | A Secret Service provider reached through the user's session D-Bus |
| Windows | `%USERPROFILE%\.magicvault` | The user's Windows Credential Manager |

Explicit initialization generates the key and a vault-instance UUID. The keyring
entry uses service `ai.magicbeans.magicvault` and account `instance-<UUID>`.
The daemon loads and caches that key in owned zeroizing memory for its lifetime;
locking the OS keyring afterward does not automatically erase a running daemon's
cached key. If the daemon cannot retrieve its existing key at startup, it refuses
to open the vault, without creating a replacement key. Linux has no volatile kernel-keyring
or plaintext-file fallback. Copying the vault directory alone does not transfer
its OS-held key to another machine.

Other files beneath the root have different roles:

| File | Contents and protection |
| --- | --- |
| `instance.json` | Vault identity, including the UUID used to find the master key. |
| `clients.json` | Client token hashes, references, policies and other broker registry metadata. |
| `client-<profile>.json` | The paired client's bearer capability. This is sensitive even though it contains no saved website password. |
| `vault/secret_audit.jsonl` | Typed audit events and operation metadata, without credential values. |

These JSON/JSONL files are protected by filesystem access controls; they are not
all encrypted vault files. macOS/Linux use owner-only directories (`0700`) and
files (`0600`). Windows uses an owner/SYSTEM-only access-control list (DACL),
rejecting unsafe ownership, permissive access and reparse points. The separate
`.magicvault-app` directory under the same user home contains managed application
versions and installation state. Keep pairing files and private runtime state
out of agent messages and shared diagnostics.

For **just-in-time input**, no credential record is created on any platform.
Values pass from the masked desktop helper to the daemon and the approved browser
delivery adapter in temporary memory. They are omitted from vault records,
registry state, audit events and agent replies. Owned secret buffers are zeroized
when dropped; OS/UI/browser copies are outside that guarantee. See
[one-time credentials](jit-credentials.md#lifetime-and-boundary).

## Local IPC and credential flow

The model-facing MCP connection uses **stdio**. The native MCP server and CLI
then connect to the local daemon; the npm SDK invokes that CLI. The daemon's
control API does not open a TCP/HTTP listener.

- **macOS/Linux:** `rpc.sock` carries client requests and `bridge.sock` carries
  trusted browser-native-host traffic under the private vault root. Both sockets
  use `0600` permissions, and each side checks that the peer has the same Unix
  user ID. The vault root retains the 85-byte path limit for Unix socket paths.
- **Windows:** the same logical endpoints map to separate
  `\\.\pipe\MagicVault-<hash>` named pipes. The hash binds the canonical root,
  endpoint and user SID. Pipes use an owner/SYSTEM-only DACL, reject remote
  clients and reserve the first server instance. Both sides also verify the
  other process's user SID. No `.sock` filesystem transport is used on Windows.

OS peer checks are combined with a paired-client capability and per-client
authorization for protected operations. Protocol messages use length-prefixed
JSON, enforce size limits before allocating their payloads and have bounded
admission/timeouts. Version and daemon-session checks reject incompatible or
stale requests. A lost reply never causes automatic replay of a credential use.

Credential input takes a separate path: the daemon launches its bundled
`magicvault-prompt` helper and exchanges bounded messages through private
stdin/stdout pipes. The helper returns entered values only to the daemon. Agent
requests carry references or JIT field metadata; replies contain metadata and
status. The trusted browser bridge or loopback CDP connection does carry the
values needed for the approved fill, and the destination page receives them.
There is no terminal/chat input fallback if the desktop helper fails.

These controls separate the supported agent protocol from secret delivery; they
do not sandbox other software running as the same OS user. See the
[security boundary](../SECURITY.md#what-this-does-not-protect-against) for that
limit, and [prompt behavior](prompts.md) for collection and cancellation.

Implementation references: [key identity and loading](../magicvault-service/src/storage.rs),
[vault encryption](../magicvault-core/src/encryption.rs),
[IPC framing and admission](../magicvault-service/src/ipc.rs), and
[Unix/Windows transport](../magicvault-primitives/src/local_ipc.rs).

## Installation and recovery differences

Unix custody files use owner-only permissions. Windows roots/files use an
owner/SYSTEM-only DACL; unsafe files and reparse points are refused. Windows
publishes flushed file contents with a write-through replacement. It has no
unprivileged Unix directory-fsync equivalent; power-loss durability must be
qualified separately on the target filesystem.

Unix activation replaces the `current` symlink atomically. Windows uses a
directory junction, which does not need Developer Mode or administrator symlink
privileges. Replacement occurs only after draining the daemon, but includes a
short interval without `current`. The verified old junction is detached and its
empty directory removed; its target version's files are preserved. An interrupted
activation retains complete
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

The [0.9.0 MCP demo](demo.md) records the new desktop UI on macOS: pairing,
optional login/card enrollment, browser connection and a successful one-time
login with human input/approval. This is one debug source build on one host;
it does not qualify Windows/Linux desktop behavior or a signed package. The
historical 0.8.3 demo covers the previous renderer. The platform build/test
workflow and focused tests cover the source changes. Record executed results in the
[qualification index](qualification/README.md) before marking desktop acceptance
complete. Windows scheduled tasks, Credential Manager, browser registration,
Linux login-session startup/keyring and headed UI behavior require native runs.
