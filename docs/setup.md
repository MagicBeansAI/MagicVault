# Standalone setup (macOS alpha)

For prebuilt CLI/MCP packages without a Rust toolchain, start with
[installation and lifecycle](distribution.md) or the [quick start](../README.md#quick-start).
`setup` creates a stable application installation separate from the vault;
`doctor`, `upgrade` and `uninstall` manage that installation explicitly.
The foreground/source workflow below remains supported.

Builds, automated tests, disposable Chrome/CLI qualification and one installed
macOS/Chrome keychain/native-consent workflow
[pass within the recorded scope](qualification/results-discovery-2026-09-08.md).
Broader native recovery and permission cases remain manual gates. Automated CLI
tests use test-only human/key providers, not a live credential store.
Start with synthetic data and disposable profiles. The crates use Rust edition
2021; the separate Rust 1.88+ compiler requirement comes from the standalone
MCP package's exact `rmcp 3.1.0` SDK dependency. The edition is not the compiler
version. See the [toolchain explanation](#rust-toolchain), [testing](testing.md) and
[security](../SECURITY.md) for the current evidence and limits.

## Rust toolchain

This is the developer/source path. Prebuilt candidate users do not need Rust;
use [installation and lifecycle](distribution.md) instead.

MagicVault's crates use **edition 2021**. The standalone MCP package requires
compiler **1.88+** because of its exact `rmcp 3.1.0` dependency. An edition selects
language rules, not a compiler version. See Cargo's
[edition](https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field)
and [compiler-version](https://doc.rust-lang.org/cargo/reference/rust-version.html)
documentation. Recorded builds used Rust 1.92.0; they are not a separate test of
the minimum compiler. [Component versions](versioning.md).

```bash
git clone https://github.com/MagicBeansAI/MagicVault.git
cd MagicVault
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
make build-standalone
export PATH="$CARGO_TARGET_DIR/release:$PATH"
export MAGICVAULT_EXAMPLES="$PWD/examples"
magicvault --version
```

Continue with the foreground service below, then pair/enroll. For MCP, use the
absolute `$CARGO_TARGET_DIR/release/magicvault-mcp` executable rather than a
packaged app path. Do **not** run packaged `setup` against a bare binary directory;
it requires a complete bundle. Source extension users also need
`make package-extension` and the [explicit native-host install](browser-usage.md#chromium-extension).
Keep installed executables at a stable trusted location; do not disconnect the
build volume while a service/native host still uses binaries from it.

## Build and foreground service

When build/verification is authorized, follow the focused lanes in
[testing](testing.md), then use `make build-standalone`.
The Makefile exports `CARGO_TARGET_DIR`; binaries are under its `release/`.
Use `make -s print-target-dir` to locate them. Builds prefer an available SSD1
volume and otherwise use `target/`; see [build location and overrides](testing.md#build-and-test-artifact-location).
Put `magicvault`, `magicvault-mcp`, and (for the extension)
`magicvault-native-host` in a stable, trusted executable location.
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
Enrollment does not activate delivery. Browser use requires the explicit
client/field/origin configuration and per-fill human consent described in
[browser usage](browser-usage.md). New HTTP/process use requires a separately
registered fixed [delivery profile](delivery-usage.md) and fresh per-use consent.
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

MCP is the recommended routine interface for local agents. Start with the
[Codex and Claude Code quick start](../README.md#use-with-an-mcp-agent); use the
[CLI](../README.md#cli-for-agents-and-scripts) for shell-based agents/scripts, or
the [builder guide](integrations.md) for embedding. MCP is local stdio, not a
hosted HTTP endpoint. It does not remove human setup or per-use consent.

The README's commands use the default application/vault paths and paired profile
`agent`. Claude Code's `--scope user` makes the MCP entry available across
projects; use `--scope local` for a project-local entry instead. After connecting,
start a new session and inspect `/mcp`. `codex mcp list` or `claude mcp get magicvault`
shows configured servers; an entry alone does not prove daemon readiness or
pairing. Check `vault_status` and `list_credentials` through the agent too.

Normal packaged `setup` prints a `mcpServers` object with the installed executable
and root/profile arguments. For clients that accept that JSON shape, copy it into
their personal configuration. A custom-path example is:

```json
{
  "mcpServers": {
    "magicvault": {
      "command": "/absolute/app-directory/current/bin/magicvault-mcp",
      "args": ["--root", "/absolute/vault-root", "--profile", "agent"]
    }
  }
}
```

Replace the placeholders with the values printed by setup; JSON does not perform
shell expansion of `$HOME` or `$(...)`. Codex uses its own TOML configuration, so
use its `codex mcp add` command rather than pasting this JSON into `config.toml`.
For source builds, use the resolved `$(make -s print-target-dir)/release/magicvault-mcp`
path instead. Never copy pairing capabilities or credentials into client settings.
Pair/enroll using the human CLI first. The MCP process is a client of the already
running daemon, not an alternative store owner or a daemon auto-installer.
`--profile` selects a MagicVault client pairing, not a Chrome browser profile.
Two agents using that same pairing share its permissions and handles; use
separately paired/authorized clients when isolation is required. The native-host
registration selects one client profile, so another client does not automatically
see its connected extension browsers.

After connecting, ask the agent to call `vault_status` and `list_credentials`.
Missing browser/destination metadata calls for human setup, never reading a secret
or making up a destination. Client-side tool approvals and MagicVault's native
approval are separate; neither authorizes bypassing the other. This is not an
unattended CI interface. Client commands/formats are documented by
[OpenAI](https://developers.openai.com/codex/mcp) and
[Anthropic](https://code.claude.com/docs/en/mcp).

The advertised tools are `vault_status`, `list_credentials`, `request_approval`,
`approval_status`, `list_browsers`, `browser_targets`, `secure_fill`, `fill_status`
and `cancel_fill`, plus `list_delivery_profiles`, `secure_new_process`,
`secure_new_http`, `delivery_status` and `cancel_delivery`. No pairing, enrollment,
destination-profile registration/removal, browser registration, policy editing,
shutdown, human-grant, raw-material or unimplemented effect method is an MCP tool.
Browser setup stays human-facing. Use the same paired profile for that setup and
the MCP executable; handles and permissions are client-scoped.
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

## Upgrade and recovery

CLI/MCP/service packages use source version `0.7.0`; protocol/effect crates use
`0.5.0`, with agent wire version `3` retained. Extension `0.6.0` uses native connection handshake `2`: rebuild/update
host and daemon together. Effect schemas and host config remain `1`. Normal
packaged setup now installs the exact bundled native-host identity after pairing;
install-only does not. See [one-time unpacked migration](browser-usage.md#upgrading-older-unpacked-extensions).
This is a source alpha with local prebuilt npm packaging, not an announcement of
published registry packages or Apple-verified releases. See the [component version matrix](versioning.md).
Upgrade `magicvault`, `magicvault-mcp` and `magicvault-native-host` together and
restart the standalone daemon. Older/newer mismatched wire versions fail closed;
there is no automatic downgrade or transport fallback. Existing vault framing,
instance/key identity and core/primitives versions are unchanged.

The standalone registry retains browser permissions and gains delivery profiles,
initially empty for existing installations. No enrolled credential gains new
effect authority on upgrade. Older standalone daemons may reject a registry
containing new permission/profile fields. Do not run one over that registry or remove policy data to force a
downgrade; use a deliberately reconciled, consistent backup if rollback is needed.
Embedded core consumers such as Magician do not read this standalone registry or
link the new browser/transport packages.

Delivery profiles persist. Browser connections, target handles, pending consent and effect status are
ephemeral and invalid after restart. Approved extension profiles reconnect
automatically; clients must rediscover fresh handles and must not replay effects.
Missing status after restart is not safe-to-retry evidence. A completed durable
receipt may help a trusted operator reconcile, but no receipt can prove that a
lost browser, child-process or HTTP reply meant no side effect occurred.

`persistence_uncertain` means a write may have committed. The daemon fails closed
until restart/reconciliation; it does not roll memory back optimistically or run
the effect twice. Retain state for inspection. Keychain absence, corruption,
wrong owner/mode, bad instance format, and a second writer fail closed. No
in-memory key fallback or production auto-approve mode exists. A denied operation
must not trigger raw-secret fallback. See [protocol.md](protocol.md).

Unsafe existing audit journals (including symlinks and non-private files),
quarantine evidence (`vault/*.corrupt-*`), ACLs referencing missing material,
or a missing registry beside existing material prevent a healthy empty restart.
Restore a consistent instance/key/vault/registry backup or reconcile offline with
a trusted operator. No automatic quarantine deletion, orphan adoption, lost-key
recovery, or repair tool is currently shipped. Do not deploy over valuable
credentials before these recovery procedures and the native host are qualified.
