# Standalone foundation setup (macOS, unqualified)

These are procedures to execute **after verification is permitted**, not results
of this implementation. No binary, native dialog, keychain operation, installer,
daemon, CLI, MCP or test was run during development. Rust 1.88+ is required by
the exact `rmcp 3.1.0` SDK dependency. Source repositories may remain private;
builders need Git read access, running binaries do not need GitHub access.
The [follow-up static review](static-review-2026-09-06.md) advances core to
`0.1.2`, primitives to `0.1.1`, and service/CLI/MCP to `0.2.1` without changing
protocol version, vault format, root or key identity. It is still unqualified.

## Build and foreground service

When allowed, use `make check`, `make test`, then `make build-standalone`.
The Makefile exports `CARGO_TARGET_DIR`; binaries are under its `release/`.
Put `magicvault` and `magicvault-mcp` in a stable, trusted executable location.
The examples assume that location is already on PATH.

```sh
magicvault init
magicvault serve
```

Default standalone root: `$HOME/.magicvault`. A custom `--root` must be absolute,
private and short enough for a Unix socket (root at most 85 bytes). Setup only
creates a fresh/recognized root; it refuses an unrelated nonempty directory.
Never use Magician's root. Existing keys are never silently regenerated.
The foreground process exits cleanly on SIGINT/SIGTERM. Use another terminal for
the following commands; stdout/status never carries enrolled values.

## Pair and enroll through human prompts

```sh
magicvault --profile owner pair --label 'My owner terminal'
magicvault --profile owner enroll --label 'Example account' --field username --field password
magicvault --profile owner list-credentials
magicvault --profile agent pair --label 'My automation client'
magicvault --profile agent request-access --credential-ref cred_REPLACE_WITH_RETURNED_UUID
magicvault --profile agent approval-status --approval-id REPLACE_WITH_RETURNED_UUID
magicvault --profile agent list-credentials
```

Pairing and enrollment request native dialogs owned by the daemon. Deny/Cancel
is the default. Enter values only in hidden prompts, never command arguments,
labels, field names, logs or chat. Approving metadata access exposes the selected
reference/label/field names to that client—not values or future delivery authority.
Enrollment's delivery policy is inactive for browser/HTTP/process use; the next
adapter phase must add its own destination-bound policy and consent.
The initial hidden-input backend accepts nonempty UTF-8 text, at most 4096 bytes
per field, up to eight fields, within one 180-second enrollment window. The
deadline is rechecked after writer/audit waits immediately before persistence.
Binary and multiline credential import are not supported setup paths.

Only one human interaction runs at a time. `busy` means wait for that interaction
to finish; no hidden unbounded prompt queue exists. Poll a returned approval ID
for completion. After restart, a missing approval is `not_found`, not evidence
of permission to use a credential; durable metadata access is visible in the
permitted list. Approval IDs are client-scoped and expire.

Pairing capabilities are saved in `client-PROFILE.json`, mode 0600, and are not
printed. Do not share these files or pass their contents to a model. A partial
pairing-file write is retained and refused, never overwritten automatically.
If pairing is uncertain, stop and reconcile that specific client with the owner;
do not retry enrollment/mutations blindly or delete a vault/key to repair setup.

## MCP

Configure a stdio MCP server with the absolute path to `magicvault-mcp` and args
`["--profile", "agent"]`; include `--root` only for a custom standalone root.
Pair/enroll using the human CLI first. The process is a client of the already
running daemon, not an alternative store owner.

The advertised tools are exactly `vault_status`, `list_credentials`,
`request_approval` and `approval_status`. No pairing, enrollment, shutdown,
human-grant, raw-material or unimplemented effect method is an MCP tool.
The client bounds runtime teardown after SDK completion to handle blocked stdio;
the separate custody daemon continues to drain durable writes without that bound.

## User-session service

```sh
magicvault service install
magicvault service start
magicvault status
magicvault service stop
magicvault service remove
```

Installation writes a per-instance LaunchAgent definition without overwriting
an existing file. `start` loads it; query status separately for actual readiness.
It loads at login, only in an Aqua user session, and does not restart endlessly
after key/backend failure. `stop` unloads it, allowing SIGTERM drain. To restart,
stop/unload then start; no forced process replacement or automatic retry occurs.
`remove` deletes only the managed definition and does **not** claim to stop an
already running process. Stop first. Removal leaves vaults, pairings and keys
unchanged; reinstalling recreates the definition.

For a foreground daemon, `magicvault --profile owner stop` requests native human
consent to shut down, or use SIGINT/SIGTERM. `revoke-client --client-id UUID`
requires a paired caller plus native human consent; revocation blocks future
access through that capability and does not erase material or revoke providers.

## Failure and recovery boundaries

`persistence_uncertain` means a write may have committed. The daemon fails closed
until restart/reconciliation; it does not roll memory back optimistically or run
the effect twice. Retain state for inspection. Keychain absence, corruption,
wrong owner/mode, bad instance format, and a second writer fail closed. No
in-memory key fallback or production auto-approve mode exists. A denied operation
must not trigger raw-secret fallback. See [protocol.md](protocol.md).

Quarantine evidence (`vault/*.corrupt-*`), ACLs referencing missing material,
or a missing registry beside existing material prevent a healthy empty restart.
Restore a consistent instance/key/vault/registry backup or reconcile offline with
a trusted operator. No automatic quarantine deletion, orphan adoption, lost-key
recovery, or repair tool is shipped in this phase. Do not deploy over valuable
credentials before these recovery procedures and the native host are qualified.
