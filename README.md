<div align="center">
  <h1>MagicVault</h1>
  <p><strong>Keep secrete away from Agents</strong></p>
  <p>
    <a href="CHANGELOG.md"><img src="https://img.shields.io/badge/source-v0.6.0%20alpha-7C3AED.svg" alt="Source version 0.6.0 alpha" /></a>
    <a href="#quick-start"><img src="https://img.shields.io/badge/standalone-macOS-lightgrey.svg" alt="Standalone host: macOS" /></a>
    <a href="#rust-toolchain"><img src="https://img.shields.io/badge/Rust-2021%20edition-orange.svg" alt="Rust language edition 2021" /></a>
    <a href="#rust-toolchain"><img src="https://img.shields.io/badge/compiler-1.88%2B-orange.svg" alt="Standalone MCP compiler requirement: Rust 1.88 or newer" /></a>
    <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="MIT or Apache-2.0 license" /></a>
  </p>
  <p>
    <a href="#quick-start">Quick start</a> ·
    <a href="#what-works-today">Coverage</a> ·
    <a href="#use-with-an-mcp-agent">MCP</a> ·
    <a href="#cli-for-agents-and-scripts">CLI</a> ·
    <a href="#build-on-magicvault">For builders</a> ·
    <a href="docs/architecture.md">Architecture</a> ·
    <a href="SECURITY.md">Security</a>
  </p>
</div>

Give an agent access to an account without putting its password into a prompt
or typing-tool argument. MagicVault stores the credential, lets the agent select
a reference, asks you to approve its use, and delivers it to an authorized browser
field, new command-line program or HTTP request. The agent receives status—not
the credential value or a recipient's raw output.

- **Keep your browser workflow.** Your automation tool owns navigation and
  submission; MagicVault connects to the same browser just for credential fills.
- **Connect your agent once.** MCP exposes `secure_fill`, `secure_new_process`
  and `secure_new_http` as discoverable tools. Your agent handles the request
  and status polling; you approve the actual use in MagicVault's native dialog.
- **Approve the actual destination.** Browser fills bind client, fields, origins
  and document. New commands/HTTP bind a fixed recipient profile. Every use
  requires fresh consent; permission to list a vault is not delivery authority.

## Choose your interface

| You want to… | Start with | Why |
| :--- | :--- | :--- |
| **Use MagicVault from Codex, Claude Code or another local MCP client** | **[MCP quick start](#quick-start) — recommended** | Discoverable, reference-only tools; no manual request files for routine agent use |
| **Call MagicVault from a shell-capable agent, Bash script or tool wrapper** | **[CLI](#cli-for-agents-and-scripts)** | Explicit commands and JSON receipts for the same secure operations |
| **Build credential delivery into your application or extension** | **[Developer integrations](#build-on-magicvault)** | Rust crates, authenticated local protocol and native extension bridge; no dedicated Python/Node SDK yet |

We recommend MCP for **day-to-day agent use**, not unattended credential
access. One-time installation, enrollment and destination setup remain human
steps, and every delivery still needs native consent. The CLI provides those
administrative steps as well as an alternative automation interface. MCP is local
stdio here—not a hosted service or a remote URL to paste into a cloud-only client.

> [!WARNING]
> **Early-access software.** Start with synthetic credentials and disposable
> profiles. Real CDP/CLI tests pass on the recorded macOS/Chrome setup;
> installed-extension/native-keychain acceptance remains open.
> [Test evidence and limits](docs/testing.md).

## What works today

The destination matters as much as the interface. Browser fills attach to an
existing session; process and HTTP operations create a **new, fixed-profile**
invocation. They do not offer arbitrary secret-bearing commands or URLs.

| Destination / use case | Available today? | Connection, conditions and limits |
| :--- | :--- | :--- |
| **Existing browser/session (stateful), headed or modern headless** | **Yes — direct CDP fills** | Chrome/Chromium must expose a supported **loopback browser debugging websocket**. MagicVault opens a second connection; no extension or proxy is needed. The existing tool keeps the browser/session. |
| **Already-running headed browser without CDP** | **Implemented — extension; native acceptance pending** | Install the MagicVault Chromium extension **and native host**, run the daemon, and grant the target sites. No debugging port needed. The macOS host installer supports Chrome/Chromium. |
| **Browser accessible only through a driver's private pipe, or a remote CDP endpoint** | **Not directly** | Configure an accessible local browser websocket, or use the headed extension path where installation is possible. An arbitrary browser/driver cannot be attached automatically. |
| **Embedded browser frames** | **Conditional** | CDP supports frames addressable through the attached page session. The extension uses explicit document/frame targeting and site permissions. Both top-page and frame origins must be allowed; opaque origins are refused. Broader cross-process frame compatibility is not claimed. |
| **New HTTP(S) requests** — `GET`, `HEAD`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `TRACE` and supported custom methods | **Yes — `secure_new_http`** | Human-registered exact URL/method and header/query/text/form/flat-JSON credential placements. Public HTTPS; plain HTTP only to explicit loopback IPs. No CONNECT, redirects, ambient proxy, retries, custom CA or insecure TLS switch. Responses, headers and raw status codes are withheld. |
| **New processes / command-line programs** | **Yes — `secure_new_process`** | Human-registered executable/arguments/cwd, environment and/or stdin slots; fresh consent each run. MagicRun owns bounded batch execution and cleanup. Executable bytes are checked against the registered digest. No credential arguments, interactive terminal or raw stdout/stderr/exit-code return. |
| **Already-running processes / interactive terminals** | **Not implemented** | Requires a cooperating input, IPC or credential-provider integration. There is no generic “inject into this PID” or live environment-rewrite capability. |
| **Stateful services / long-lived API clients** | **Not implemented** | Credential refresh, connection pools and existing sessions need an explicit service/provider adapter. Synthetic service-rotation fixtures are preparation, not a working product integration. |
| **Native application password fields / non-Chromium browsers** | **Not implemented** | No accessibility, OS-level secure typing, Firefox or Safari adapter is provided. |
| **Other tools' DOM reads, screenshots, cookies and session output** | **Not filtered** | Browser delivery does not prevent a separate tool from observing secrets afterward. Password masking is not an observation filter. |

Browser fills require a supported writable input, a current document-bound
target, explicit field/origin permission and human approval for each use.
Headless Chrome still needs the daemon's interactive human-approval host.
A new browser follows the same rules once your tool launches it; MagicVault
does not launch or take ownership of browsers.
See [browser conditions and supported controls](docs/browser-usage.md).

## Quick start

**Recommended: connect your agent through MCP.** Install and authorize once, then
let the agent select references and call the secure tools. No Rust toolchain is
needed on the prebuilt path; [source builds](#rust-toolchain) are for developers.

Prebuilt packages need **macOS Apple Silicon and Node.js 22+**, not Rust.
Native approval dialogs require a logged-in desktop session. Chrome/Chromium is
needed only for browser fills. Packaging is implemented; **npm publication and
Apple-signed/notarized releases have not been performed**. Do not treat an
unsigned candidate as a verified public release.

### 1. Install matching release-candidate tarballs

Given the two local tarballs produced by the [distribution workflow](docs/distribution.md):

```bash
# These are LOCAL candidate filenames, not a claim of npm registry availability.
export MAGICVAULT_NPM_DIR="$HOME/.local/share/magicvault-npm"
npm install --prefix "$MAGICVAULT_NPM_DIR" --ignore-scripts \
  ./magicvault-local-magicvault-darwin-arm64-0.6.0.tgz \
  ./magicvault-local-magicvault-0.6.0.tgz
export PATH="$MAGICVAULT_NPM_DIR/node_modules/.bin:$PATH"
magicvault --version
```

The launcher has no install hooks and does not download executables at runtime.
Keep npm's optional dependencies enabled. Package installation alone never
initializes a vault, pairs an agent or starts the daemon.

### 2. Set up the vault and your agent profile

```bash
magicvault --profile agent setup
magicvault --profile agent doctor
magicvault --profile agent enroll --label 'Demo account' --field password
magicvault --profile agent list-credentials
```

`setup` explicitly copies executables/extension assets into `~/.magicvault-app`,
initializes the separate `~/.magicvault` and its keychain identity, starts the
user-session service, and requests native pairing consent. It prints a ready-to-copy
`mcpServers` configuration using a **stable absolute executable path**, without
capability tokens. npm/npx cache cleanup cannot remove that installed service.
Enter a synthetic password only in the hidden native prompt—not in chat or the
shell. [Setup, upgrades, removal and recovery](docs/distribution.md#setup-and-lifecycle).

<a id="use-with-an-mcp-agent"></a>

### 3. Connect your agent

Run the command for your client after normal `setup`. These examples use the
default application/vault directories and paired profile `agent`:

**Codex**

```bash
codex mcp add magicvault -- "$HOME/.magicvault-app/current/bin/magicvault-mcp" \
  --profile agent
codex mcp list
```

This uses Codex's local MCP configuration. See [official Codex MCP setup](https://developers.openai.com/codex/mcp).

**Claude Code**

```bash
claude mcp add --transport stdio --scope user magicvault -- \
  "$HOME/.magicvault-app/current/bin/magicvault-mcp" --profile agent
claude mcp get magicvault
```

`--scope user` makes the entry available across your projects; use `--scope local`
for just the current project. See [official Claude Code MCP setup](https://code.claude.com/docs/en/mcp).

Start a new agent session and inspect `/mcp` to confirm the server is connected.
Ask it to call `vault_status` and `list_credentials` before requesting an effect.
An MCP connection alone does not prove the daemon is ready or the profile is paired.
For another stdio client, use the `mcpServers` object printed by `setup`, adapting
the client-specific configuration format. For custom directories, use that
printed absolute executable path and **both** root/profile arguments; never put
pairing tokens or enrolled values in MCP configuration. [Configuration details](docs/setup.md#mcp).

The client launches the MCP process; the separately installed daemon owns custody
and native consent. Do not run `magicvault-mcp` as a foreground vault or expect it
to install/start a daemon. Keep the managed service running. Existing client tool
approvals may apply in addition to MagicVault's consent—do not disable safeguards
to make delivery appear unattended.

### 4. Authorize your destination

For **browser fills**, choose one connection path below, then authorize the exact
credential fields and origin. Both expose the same `secure_fill` tool.
For **new commands or HTTP requests**, have the human register a fixed
[destination profile](docs/delivery-usage.md#register-once-approve-each-invocation),
then continue to step 5; neither CDP nor an extension is needed.

<details>
<summary><strong>CDP — an automation browser with a debugging websocket</strong></summary>

Use the loopback **browser websocket** reported by your trusted browser launcher.
Register it through the human CLI; do not put the endpoint into model messages:

```bash
magicvault --profile agent register-cdp --label 'Automation browser' \
  --endpoint ws://127.0.0.1:9222/devtools/browser/REPLACE_WITH_BROWSER_ID
```

This is a placeholder endpoint, not an HTTP discovery URL or a page websocket.
If your browser is not already configured, follow the
[disposable headed/headless launch example](docs/browser-usage.md#direct-cdp).
A browser must be started with the appropriate debugging setup; simply knowing
its tab ID is not enough. Never expose its debugging port to the network.

</details>

<details>
<summary><strong>Extension — a headed browser without a debugging port</strong></summary>

Packaged `setup` includes a stable unpacked extension directory. Source builders
can instead run `make package-extension` and use their checkout's `dist/extension`.
Do not load the raw `extension/` source directory: packaging adds the required
shared `fill.js` asset.

1. Open `chrome://extensions`, enable **Developer mode**, click **Load unpacked**,
   and choose `~/.magicvault-app/current/extension` (or the path printed by setup).
2. Normal `magicvault setup` installs the native bridge automatically for the
   bundled extension's fixed ID. No copying or pasting an extension ID is needed.
   For an existing installation, rerun setup after upgrading to select this ID:

   ```bash
   magicvault --profile agent setup
   ```

3. Choose **Allow all HTTPS websites** for one-time browser permission, or use
   **Allow site** for selected websites. Local HTTP development sites require
   separate grants. **Disable site in MagicVault** excludes a site from discovery
   and fills without changing Chrome's underlying permission. With the daemon running,
   the extension connects automatically. Match its browser profile ID in the
   first native approval dialog. Every credential fill still needs approval.

Normal setup also checks native-host registration and whether an extension profile
has connected. To check again after loading/enabling the extension:

```bash
magicvault --profile agent doctor
```

Read `extension.native_host`, `extension.connection` and `extension.next_steps`.
An unconfirmed installation does **not** mean the extension is missing: Chrome
may be closed, the extension paused, or approval/retry pending. A connection does
not grant website access or authorize fills. [Diagnostic states](docs/browser-usage.md#checking-extension-readiness).

Native-host definitions are **OS-user-wide**, not isolated by a Chrome profile.
Separate browser profiles maintain independent, remembered connections. Transient
failures retry with a 30-second to 5-minute backoff; **Pause connection** survives
browser restarts. Denial/revocation or a copied-profile conflict stops retries.
Only exact managed host definitions can be repaired; foreign files are refused.
Site permission is separate from the
vault's field/origin policy below. This path is implemented but its installed
native workflow still needs [acceptance testing](docs/qualification/extension.md).
See [extension setup, reconnect and removal](docs/browser-usage.md#chromium-extension).
Source-only installations instead use `magicvault extension install`
with `--host-executable "$CARGO_TARGET_DIR/release/magicvault-native-host"`.
Broad browser access never authorizes credentials automatically. Existing grants
are preserved with the fixed identity on future upgrades; legacy path-derived
IDs need a one-time reload/regrant migration. Enabling all HTTPS is always explicit.

</details>

For browser fills, replace the placeholders with your credential reference and
the exact origin of a trusted test page. For a local source-checkout demo, `make fixture-site` serves
[synthetic pages](docs/qualification/browser.md#reusable-pages-for-manual-and-extension-testing)
(requires Node.js); use its printed loopback origin instead of the example below.

```bash
magicvault --profile agent configure-browser-credential \
  --credential-ref cred_REPLACE_WITH_ENROLLED_UUID \
  --field password --origin https://accounts.example.com
```

### 5. Let your agent use MagicVault

After that setup, ask your agent, for example:

> Use MagicVault to discover my Demo account reference and the authorized browser
> tab. Fill its password field with `secure_fill`, then poll that operation's
> status. Never retrieve or type the credential through another tool.

For a registered command or HTTP destination:

> List my MagicVault delivery profiles, run the selected test profile using
> `secure_new_process` or `secure_new_http`, and report its delivery status.

Approve the native dialog after checking the recipient. The agent supplies
references/handles, a CSS selector for browser fills, and a fresh operation ID—not
credential values. It retains that ID for status polling. Browser navigation and
submission still belong to your existing browser tool; MagicVault does not provide
them. `filled`/`completed` is a delivery receipt, not proof of login or API success.
Never automatically replay a missing, partial or uncertain operation.

MCP exposes discovery, approval requests, secure delivery, status and cancellation.
It does **not** expose enrollment, destination registration, browser-policy editing,
self-approval or raw-secret reads. If setup is missing, the agent should ask the
human to complete it—not ask for a password in chat. [Full MCP tool catalog](docs/setup.md#mcp).

## CLI for agents and scripts

Use the CLI when your agent has shell access but no MCP support, or when you want
explicit Bash/tool-wrapper calls. MCP is not required. The same paired daemon,
destination policy and per-use native consent apply: a script is **not** an
unattended/headless-CI credential runner. The CLI also contains human administration
commands; scripts must not try to grant themselves approval.

| Operation | MCP tool | CLI command |
| :--- | :--- | :--- |
| Fill an existing browser field | `secure_fill` | `magicvault secure-fill` |
| Launch a registered new command | `secure_new_process` | `magicvault secure-new-process` |
| Send a registered new HTTP request | `secure_new_http` | `magicvault secure-new-http` |

After the same human setup above, inspect value-free metadata:

```bash
magicvault --profile agent status
magicvault --profile agent list-credentials
magicvault --profile agent list-delivery-profiles
export MAGICVAULT_EXAMPLES="$HOME/.magicvault-app/current/examples"
```

Source builders can use their checkout's `examples/` instead. Recipient output is
withheld; capture closed receipts and retain each operation ID for reconciliation.

### Browser fill from the CLI

```bash
magicvault --profile agent list-browsers
magicvault --profile agent browser-targets --browser-handle REPLACE_WITH_BROWSER_HANDLE
```

Choose the correct tab/frame from discovery. Copy the reference-only example to
a temporary file and edit it with your credential reference, browser/target
handles, an exact CSS selector, and a **fresh operation UUID** from `uuidgen`:

```bash
fill_request=$(mktemp /tmp/mv-fill.XXXXXX)
cp "$MAGICVAULT_EXAMPLES/secure-fill.json" "$fill_request"
uuidgen
open -e "$fill_request"
```

Save the file before continuing. It contains references and a selector, never
the password. For the local demo's password input, use `#password` as the selector.

```bash
magicvault --profile agent secure-fill --request-file "$fill_request"
magicvault --profile agent fill-status --operation-id REPLACE_WITH_OPERATION_UUID
```

Approve the fill's native dialog and poll that **same** operation for completion.
Target handles expire after three minutes; discover them after preparing your
test page and credential. `filled` means delivery, not successful login. Your
existing browser tool handles the next step. Never automatically repeat a
`partial` or `uncertain` operation. [Status, cancellation and recovery](docs/browser-usage.md#request-a-fill-and-inspect-its-outcome).

### New commands and HTTP requests

After pairing/enrollment, a human registers a fixed destination using the same
paired client profile. Copy **one** reference-only example and edit its paths or
URL and `credential_ref`/field. Do not insert a password into the file:

```bash
delivery_profile=$(mktemp /tmp/mv-delivery.XXXXXX)
cp "$MAGICVAULT_EXAMPLES/http-profile.json" "$delivery_profile"  # Or process-profile.json.
open -e "$delivery_profile"
# Save after reviewing the exact recipient and every credential placement.
magicvault --profile agent register-delivery-profile --request-file "$delivery_profile"
magicvault --profile agent list-delivery-profiles

operation_id=$(uuidgen)  # Choose once; keep this ID for reconciliation.
magicvault --profile agent secure-new-http \
  --profile-id REPLACE_WITH_REGISTERED_PROFILE_UUID --operation-id "$operation_id"
# For a process profile, use secure-new-process with the same two flags.
magicvault --profile agent delivery-status --operation-id "$operation_id"
```

Registration and each invocation request separate native consent. The agent can
select a profile ID, **not override its destination**. `completed` is a transport/
process receipt, not proof of application success. Output is deliberately withheld,
including encoded credential echoes. A missing or uncertain result never means
safe to retry. [Profile format, limits, cancellation and recipient trust](docs/delivery-usage.md).

## Build on MagicVault

For developers integrating credential delivery into a product, choose the trust
boundary you actually need. Prefer the reference-only daemon protocol when your
application does not need to own custody; embed the Rust core only in trusted code.

| Interface | For | What you integrate |
| :--- | :--- | :--- |
| **Daemon / local protocol** | Application and SDK builders in any language | Authenticated local IPC with shared custody, permission, consent and operation state |
| **Rust crates** | Trusted application/browser-tool builders | `magicvault-core` for custody; `magicvault-effect` for delivery; `magicvault-primitives` for shared utilities |
| **Native extension bridge** | Existing Chromium extension builders | Implement the documented bridge in your own trusted extension instead of requiring a second extension |

Rust libraries and the native bridge are trusted code that handles plaintext.
Builders must preserve the authorization and model-output boundary; importing a
crate does not make arbitrary tools safe. There are no dedicated Python/Node SDKs
yet. See the [builder guide](docs/integrations.md) and [protocol](docs/protocol.md).

Magician embeds the shared core directly; it does not need these standalone
surfaces. Core `0.1.3` and primitives `0.1.1` remain unchanged in `0.6.0`.
The standalone process adapter uses MagicRun `0.1.73` through its existing public
API; no MagicRun runtime or Magician change is required.
[Embedded-consumer compatibility](docs/integrations.md#existing-embedded-consumers).

### Rust toolchain

MagicVault's crates use **edition 2021**, like MagicRun and Magician. The separate
**Rust 1.88+ compiler requirement** comes from the standalone MCP package and its
`rmcp 3.1.0` dependency. An edition selects language rules; it is not a compiler
version. See Cargo's [edition](https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field)
and [compiler-version](https://doc.rust-lang.org/cargo/reference/rust-version.html)
documentation. The [recorded qualification](docs/qualification/results-2026-09-07.md)
used Rust 1.92.0; it is not a separate test of the minimum toolchain.

```bash
git clone https://github.com/MagicBeansAI/MagicVault.git
cd MagicVault
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
make build-standalone
export PATH="$CARGO_TARGET_DIR/release:$PATH"
export MAGICVAULT_EXAMPLES="$PWD/examples"
magicvault --version
```

Continue with [foreground daemon initialization, pairing and enrollment](docs/setup.md#build-and-foreground-service).
For MCP, use the resolved `$CARGO_TARGET_DIR/release/magicvault-mcp` path in the
client commands above instead of the packaged path. Do not run packaged `setup`
against a bare binary directory; it requires a complete bundle.
Source extension users also need `make package-extension` and the
[explicit native-host install](docs/browser-usage.md#chromium-extension).

Builds prefer `/Volumes/SSD1/magicvault/builds` when SSD1 is available and writable,
otherwise this checkout's `target/`. An explicit `CARGO_TARGET_DIR` takes
precedence; this moves build/test artifacts, not vault data.
[Build location and overrides](docs/testing.md#build-and-test-artifact-location).

## Security: the promise and its boundary

MagicVault keeps enrolled values out of its supported model-facing requests,
replies, errors and audit projections. Trusted delivery code and the receiving
website, HTTP service or child process necessarily see the credential.

It does **not** promise that nobody can ever read a credential: another browser
tool can inspect the DOM or session, a recipient can copy its inputs, and
privileged or unrestricted same-user software is outside the isolation boundary.
Browser password masking does not change that. Read [SECURITY.md](SECURITY.md)
before choosing an integration, and report vulnerabilities privately.

## Documentation and contributing

- [Architecture, trust boundaries and versioned drift baseline](docs/architecture.md)
- [Setup, service lifecycle and upgrades](docs/setup.md)
- [Browser usage and supported controls](docs/browser-usage.md)
- [New command/HTTP profiles and receipt-only results](docs/delivery-usage.md)
- [Building integrations](docs/integrations.md)
- [Tests, reusable fixtures and acceptance runbooks](docs/qualification/README.md)
- [Current results and remaining gates](docs/qualification/results-delivery-2026-09-07.md)
- [Changelog](CHANGELOG.md) and [component versions](docs/versioning.md)

Reproduce issues with synthetic data and include the affected interface, version
and browser/OS. Keep vaults, pairing files, profiles, traces and live credentials
out of commits and issue reports. See [testing](docs/testing.md) for focused and
full verification commands; native and public-site tests are explicit opt-ins.
Documentation covers technical contracts, architecture, usage and verification.
Planning journals and implementation-progress logs belong in Git history, not
the public documentation tree.

## License

MIT OR Apache-2.0.
