# MagicVault

Keep credentials out of your model's messages.

Current source version: **0.3.0 — browser-delivery alpha**. Install from source
using the instructions below; this checkpoint does not announce a registry,
installer or Chrome Web Store release. See the [changelog](CHANGELOG.md) and
[component versions](docs/versioning.md).

An agent should be able to ask “use my account” without seeing the password.
MagicVault stores credentials, exposes references instead of values, asks for
human consent, and delivers selected fields directly to an authorized browser.

**Phase 3 alpha: builds, automated tests, and disposable headed/headless Chrome
and CLI-to-Chrome qualification pass on one macOS/Chrome configuration.**
The [real-world qualification record](docs/qualification/results-2026-09-07.md)
separates passing cases from open gates. Installed-extension, native human/keychain,
broader-platform and performance qualification remain outstanding. Use synthetic
credentials and disposable profiles; CDP success does not certify an installed
extension workflow. See the [security and qualification limits](docs/testing.md).

## What problem does it solve?

Ordinary browser automation often puts a password into a typing tool's arguments,
where it can enter a model's context or tool transcript. With MagicVault, the
agent supplies a credential reference and a field locator. Trusted code handles
the value; the agent receives only delivery/approval/error status.

The website still receives the credential. A separate browser tool may later
read the DOM, take a screenshot, or inspect session cookies. Those observation
paths are **not filtered by this delivery-only integration**. MagicVault is not a
sandbox against privileged or unrestricted same-user software.
Read the [security boundary](SECURITY.md) before choosing an integration.

## One fill operation, two browser connections

| Connection | Intended environment | Setup |
| --- | --- | --- |
| Direct CDP | Already-running Chrome/Chromium automation profile; headed or modern headless | An explicit loopback browser debugging websocket; no extension or proxy |
| Chromium extension | A headed browser profile without a debugging endpoint | MagicVault extension, native host, site permissions and daemon connection |

Both expose MCP `secure_fill` and CLI `magicvault secure-fill`. The registered
browser handle chooses the backend. MagicVault need not launch the browser.
Your existing tool keeps navigation and form submission; filling does not claim
that login succeeded.

## Get started

The initial interactive host is **macOS**. Rust 1.88+ is required to build the
standalone workspace. The extension targets Chrome/Chromium 120+; platform and
browser claims remain subject to qualification.

1. Follow [installation and enrollment](docs/setup.md): build the executables,
   start the daemon, pair a client, and enter credentials in native hidden prompts.
2. Give that client [explicit browser permission](docs/browser-usage.md) for
   selected fields and exact page/frame origins. Metadata access alone cannot fill.
3. Connect either [CDP](docs/browser-usage.md#direct-cdp) or the
   [extension](docs/browser-usage.md#chromium-extension).
4. Configure [MCP](docs/setup.md#mcp), or use the CLI, to discover a target and
   request `secure_fill`. Approve the daemon's native dialog and inspect the
   operation's final status.
5. Let your existing browser tool continue. Do not automatically repeat a partial
   or uncertain fill, or fall back to retrieving the raw credential.

### Install the Chromium extension

The extension is currently an **unpacked source installation**, not a Chrome Web
Store release. It needs the MagicVault daemon and native host, but no CDP port.

1. Build the executables and extension assets from this repository:

   ```sh
   make build-standalone package-extension
   ```

   Put the release executables in a stable trusted location on PATH. Complete
   [daemon setup, pairing and enrollment](docs/setup.md) for profile `agent` first.
2. Open `chrome://extensions`, enable **Developer mode**, choose **Load unpacked**,
   and select this repository's `dist/extension` directory. Keep that directory
   stable. Open the extension's setup page and copy its extension ID.
3. Install its native bridge (macOS):

   ```sh
   magicvault --profile agent extension install \
     --extension-id REPLACE_WITH_EXTENSION_ID \
     --host-executable /absolute/trusted/bin/magicvault-native-host
   ```

   The host definitions apply to the **OS user**, not just one browser profile.
   Existing definitions are never overwritten. For acceptance testing, prefer a
   disposable OS account; a disposable Chrome profile alone does not isolate them.
4. Grant only the required sites in the extension setup page, and configure
   [matching exact-origin/field permission](docs/browser-usage.md#prepare-custody-and-browser-permission)
   for the same paired profile. Browser site permission alone cannot authorize fills.
5. With the daemon running, click **Connect**, approve the native connection
   dialog, refresh status, and use `list-browsers` / `browser-targets` before
   requesting `secure_fill`. Every fill requires its own native human decision.

For complete steps, troubleshooting, reconnect and removal, see
[extension installation and usage](docs/browser-usage.md#chromium-extension).
The [installed-extension runbook](docs/qualification/extension.md) lists the
human/native acceptance gates; do not confuse them with passing CDP tests.

A request contains references, never values:

```json
{
  "operation_id": "22222222-2222-4222-8222-222222222222",
  "browser_handle": "33333333-3333-4333-8333-333333333333",
  "target_handle": "44444444-4444-4444-8444-444444444444",
  "fields": [{
    "css": "input[name=password]",
    "credential_ref": "cred_11111111-1111-4111-8111-111111111111",
    "credential_field": "password"
  }]
}
```

These are illustrative IDs. Discover actual browser/target handles and use a new
operation UUID once. A target handle is single-use and document-bound. Another
tool's snapshot reference such as `@e12` is not a CSS selector or a MagicVault
handle. See the [full CLI and agent workflow](docs/browser-usage.md).

## Choose an integration surface

| Surface | For | Responsibility |
| --- | --- | --- |
| `magicvault-core` | Trusted Rust application builders | Custody, encryption, references, existing policy/grants and scoped stores |
| `magicvault-primitives` | Rust libraries needing shared low-level utilities | Durable filesystem and stack-safe JSON primitives |
| `magicvault-effect` | Trusted browser/tool builders | CDP and native-bridge delivery; host supplies authorization and consent |
| `magicvault-service` and local protocol | Application/SDK builders | Single store owner, pairing, policy, consent, jobs and audit |
| `magicvault` | People and scripts | Setup, enrollment, service management, browser setup and reference-only operations |
| `magicvault-mcp` | MCP-compatible agents | Explicit metadata, browser discovery, fill, status and cancellation tools |
| Extension/native bridge | Chromium users and extension builders | Site-permitted, document-bound fills without CDP |

See [building integrations](docs/integrations.md). Trusted libraries and the native
bridge necessarily handle plaintext; they are not raw-secret model tools.
There is no dependency on Magician or MagicRun for browser delivery.

## Current scope and longer-term goals

“Implemented” below describes source availability, not a production-safety
certification. Targeted passing tests and their limits are recorded in
[testing and qualification](docs/testing.md).

| Capability | Phase 3 implementation scope | Longer-term direction |
| --- | --- | --- |
| Encrypted custody, human enrollment, pairing, CLI/MCP | Existing foundation, retained | Wider platform/backend qualification |
| CDP credential fills | Dedicated connection; native/reactive inputs; explicit supported frames | Broader driver/frame/control compatibility |
| Extension credential fills | New extension and native host; same fill contract | More browsers and cooperating extension integrations |
| Browser authority and outcomes | Per-client field/origin policy, one-use consent, expiry, cancellation, partial/uncertain status, typed audit | Additional qualified policy/UX options |
| New HTTP requests | Not implemented | Phase 4: `secure_new_http` with destination and output mediation |
| New processes | Not implemented | Phase 4: MagicRun-backed `secure_new_process` |
| Existing stateful services/processes | Not implemented | Explicit credential-provider, refresh or IPC integrations |
| Other browser tools' outputs and session credentials | Not filtered | Separately integrated and qualified observation filtering |
| Generic CDP relay | Not required or implemented | Only if a concrete integration benefits |
| Distribution and release | Source and local packaging instructions | Qualified installers, extension distribution and release artifacts |

## Compatibility and development

Magician remains a direct shared-core consumer; it does not need the standalone
daemon, MCP, CLI or extension. Phase 3 leaves core/primitives source, versions,
vault formats and key identities unchanged. Browser permissions live in the
standalone registry, never in a consumer's runtime root.

Standalone protocol version 2 requires matching 0.3.x clients and daemon.
Read the [upgrade and recovery notes](docs/setup.md#upgrade-and-recovery) before
changing an existing standalone installation.

- [Phase 3 implementation plan](docs/phase3-browser-plan.md)
- [Static review and implementation ledger](docs/phase3-review.md)
- [Targeted check/build/test results](docs/phase3-verification-2026-09-07.md)
- [Protocol, limits and lifecycle](docs/protocol.md)
- [Focused coverage and remaining qualification](docs/testing.md)
- [Real browser, CLI, extension and process qualification runbooks](docs/qualification/README.md)
- [Historical foundation evidence](docs/phase2-foundation.md)
- [Changelog](CHANGELOG.md)
- [Versioning, compatibility and distribution status](docs/versioning.md)

Only synthetic fixtures belong in tests, examples and bug reports. Never commit
vaults, pairing capabilities, browser profiles, live configuration or credentials.
Full checks/tests and CI are opt-in; nothing in setup automatically deploys over
an existing store.

Licensed under MIT OR Apache-2.0.
