<div align="center">
  <h1>MagicVault</h1>
  <p><strong>Keep secrete away from Agents</strong></p>
  <p>
    <a href="CHANGELOG.md"><img src="https://img.shields.io/badge/source-v0.4.0%20alpha-7C3AED.svg" alt="Source version 0.4.0 alpha" /></a>
    <a href="#quick-start"><img src="https://img.shields.io/badge/standalone-macOS-lightgrey.svg" alt="Standalone host: macOS" /></a>
    <a href="#rust-toolchain"><img src="https://img.shields.io/badge/Rust-2021%20edition-orange.svg" alt="Rust language edition 2021" /></a>
    <a href="#rust-toolchain"><img src="https://img.shields.io/badge/compiler-1.88%2B-orange.svg" alt="Standalone MCP compiler requirement: Rust 1.88 or newer" /></a>
    <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="MIT or Apache-2.0 license" /></a>
  </p>
  <p>
    <a href="#quick-start">Quick start</a> ·
    <a href="#what-works-today">Coverage</a> ·
    <a href="#use-with-an-mcp-agent">MCP</a> ·
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
- **Choose your integration.** Use the CLI, expose `secure_fill`,
  `secure_new_process` and `secure_new_http` through MCP,
  or build on the Rust libraries and local daemon protocol.
- **Approve the actual destination.** Browser fills bind client, fields, origins
  and document. New commands/HTTP bind a fixed recipient profile. Every use
  requires fresh consent; permission to list a vault is not delivery authority.

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

You need **macOS, Rust 1.88+ and `make`**, plus Chrome/Chromium for browser fills.
Build from source; there is no installer or Chrome Web Store package in this
workflow. Native approval dialogs require a logged-in desktop session.

### Rust toolchain

MagicVault's crates use **edition 2021**, like MagicRun and Magician. The separate
**Rust 1.88+ compiler requirement** comes from the standalone MCP package and its
`rmcp 3.1.0` dependency. An edition selects language rules; it is not a compiler
version. See Cargo's [edition](https://doc.rust-lang.org/cargo/reference/manifest.html#the-edition-field)
and [compiler-version](https://doc.rust-lang.org/cargo/reference/rust-version.html)
documentation. The [recorded qualification](docs/qualification/results-2026-09-07.md)
used Rust 1.92.0; it is not a separate test of the minimum toolchain.

### 1. Build and start the vault

```bash
git clone https://github.com/MagicBeansAI/MagicVault.git
cd MagicVault
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
make build-standalone
export PATH="$CARGO_TARGET_DIR/release:$PATH"

magicvault --version
magicvault init
magicvault serve
```

`init` creates a private standalone vault at `~/.magicvault` and a macOS keychain
identity. It is not a read-only action. Keep it separate from another product's
data; use a disposable OS account for native acceptance testing. Leave `serve`
running in this terminal. [Custom roots and service setup](docs/setup.md).

Builds prefer `/Volumes/SSD1/magicvault/builds` when SSD1 is available and writable,
otherwise this checkout's `target/`. `make print-target-dir` shows the selected
location; an explicit `CARGO_TARGET_DIR` takes precedence. This moves build/test
artifacts, not your vault. [Build location and overrides](docs/testing.md#build-and-test-artifact-location).

### 2. Pair and enroll a credential

In another terminal, enter the same checkout and add its binaries to PATH:

```bash
export CARGO_TARGET_DIR="$(make -s print-target-dir)"  # From the MagicVault checkout.
export PATH="$CARGO_TARGET_DIR/release:$PATH"
magicvault --profile agent pair --label 'My automation client'
magicvault --profile agent enroll --label 'Demo account' --field password
magicvault --profile agent list-credentials
```

Approve the native prompts and enter a **synthetic password in the hidden input**,
never in the shell, chat or request JSON. Keep the returned `credential_ref` for
the next steps. Pairing and metadata access alone do not authorize a fill.

For process/HTTP delivery only, skip the browser steps and continue to
[new commands and HTTP requests](#new-commands-and-http-requests). Neither CDP
nor an extension is needed for those operations.

### 3. Connect your browser

Choose one path. Both give the agent the same `secure_fill` operation.

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

Package the extension from the same checkout:

```bash
make package-extension
```

1. Open `chrome://extensions`, enable **Developer mode**, click **Load unpacked**,
   and choose this checkout's `dist/extension`. Keep that directory stable.
2. Open the extension's setup page and copy its extension ID.
3. Install the native bridge, using the exact ID and a trusted absolute executable
   path (the quick-start build puts it under the selected target's `release/`):

   ```bash
   magicvault --profile agent extension install \
     --extension-id REPLACE_WITH_EXTENSION_ID \
     --host-executable "$CARGO_TARGET_DIR/release/magicvault-native-host"
   ```

4. Grant only the required sites on the setup page. With the daemon running,
   click **Connect**, approve the native connection dialog, and refresh status.

Native-host definitions are **OS-user-wide**, not isolated by a Chrome profile.
Existing definitions are not overwritten. Site permission is separate from the
vault's field/origin policy below. This path is implemented but its installed
native workflow still needs [acceptance testing](docs/qualification/extension.md).
See [extension setup, reconnect and removal](docs/browser-usage.md#chromium-extension).

</details>

### 4. Authorize the site and request a fill

Replace the placeholders with your credential reference and the exact origin of
a trusted test page. For a local demo, `make fixture-site` serves
[synthetic pages](docs/qualification/browser.md#reusable-pages-for-manual-and-extension-testing)
(requires Node.js); use its printed loopback origin instead of the example below.

```bash
magicvault --profile agent configure-browser-credential \
  --credential-ref cred_REPLACE_WITH_ENROLLED_UUID \
  --field password --origin https://accounts.example.com

magicvault --profile agent list-browsers
magicvault --profile agent browser-targets --browser-handle REPLACE_WITH_BROWSER_HANDLE
```

Choose the correct tab/frame from discovery. Copy the reference-only example to
a temporary file and edit it with your credential reference, browser/target
handles, an exact CSS selector, and a **fresh operation UUID** from `uuidgen`:

```bash
fill_request=$(mktemp /tmp/mv-fill.XXXXXX)
cp examples/secure-fill.json "$fill_request"
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

## New commands and HTTP requests

After pairing/enrollment, a human registers a fixed destination using the same
paired client profile. Copy **one** reference-only example and edit its paths or
URL and `credential_ref`/field. Do not insert a password into the file:

```bash
delivery_profile=$(mktemp /tmp/mv-delivery.XXXXXX)
cp examples/http-profile.json "$delivery_profile"  # Or process-profile.json.
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

## Use with an MCP agent

After pairing, enrollment and destination setup, configure your agent's stdio MCP
client with the executable and arguments below. Adapt the enclosing configuration
format to your client; use an absolute path to the built binary:

```json
{
  "mcpServers": {
    "magicvault": {
      "command": "/absolute/path/to/magicvault-mcp",
      "args": ["--profile", "agent"]
    }
  }
}
```

Keep the daemon running. The agent can discover credential references and browser
targets and delivery profiles, request `secure_fill`, `secure_new_process` or
`secure_new_http`, and inspect or cancel the resulting operation.
It cannot enroll raw values, register a destination, grant itself permission, edit browser policy or
read the credential. Use the same paired profile and, if customized, `--root`
for setup and MCP. [MCP tool catalog and setup](docs/setup.md#mcp).
The built executable is at `$(make -s print-target-dir)/release/magicvault-mcp`;
resolve that path in your shell, not literally inside the JSON configuration.

## Build on MagicVault

| Interface | For | What you integrate |
| :--- | :--- | :--- |
| **CLI** — `magicvault` | People, scripts and tool wrappers | Setup, enrollment, browser connections, delivery profiles, all three secure operations and status |
| **MCP** — `magicvault-mcp` | MCP-compatible agents | Reference-only discovery and browser/process/HTTP operations; no administration or raw-secret tool |
| **Daemon / local protocol** | Application and SDK builders in any language | Authenticated local IPC with shared custody, permission, consent and operation state |
| **Rust crates** | Trusted application/browser-tool builders | `magicvault-core` for custody; `magicvault-effect` for delivery; `magicvault-primitives` for shared utilities |
| **Native extension bridge** | Existing Chromium extension builders | Implement the documented bridge in your own trusted extension instead of requiring a second extension |

Rust libraries and the native bridge are trusted code that handles plaintext.
Builders must preserve the authorization and model-output boundary; importing a
crate does not make arbitrary tools safe. There are no dedicated Python/Node SDKs
yet. See the [builder guide](docs/integrations.md) and [protocol](docs/protocol.md).

Magician embeds the shared core directly; it does not need these standalone
surfaces. Core `0.1.3` and primitives `0.1.1` remain unchanged in `0.4.0`.
The standalone process adapter uses MagicRun `0.1.73` through its existing public
API; no MagicRun runtime or Magician change is required.
[Embedded-consumer compatibility](docs/integrations.md#existing-embedded-consumers).

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
