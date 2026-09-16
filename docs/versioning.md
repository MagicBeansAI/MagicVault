# Versions, compatibility and distribution

## Current component versions

**0.9.0 — one-time browser credentials, 2026-09-16 (source candidate).**
The [one-time input guide](jit-credentials.md) documents native collection without
saving credentials and the automated validation boundary. Existing native
[consent](qualification/results-consent-2026-09-08.md),
[cancellation](qualification/results-cancellation-2026-09-08.md) and
[process-launch](qualification/results-native-spawn-2026-09-08.md) records describe
their dated builds, not acceptance of the new one-time windows.
A public source repository is not a registry publication,
binary release, extension store listing or production-safety certification.

| Component | Source version / contract | Compatibility responsibility |
| --- | --- | --- |
| `magicvault` CLI, daemon and native-host executable | Package `0.9.0` | Matching wire-5 bundle; CLI-only consent inspection/revocation; native-host forwarding |
| `magicvault-mcp` | Package `0.9.0` | Matching value-free client; no tool can grant consent |
| `magicvault-service` | `0.9.0` | One-time native input, exact-use consent, bounded durable grants and revocation; destination, installation and custody checks retained |
| `magicvault-effect` | `0.7.0` | Public protocol dependency advances; runtime adapters retained |
| `magicvault-protocol` | `0.7.0` | Closed prompt-and-fill request; existing request framing and receipts retained |
| Chromium extension | Manifest `0.6.1` | Permission-filtered, loaded-tab discovery with exact narrowing; document binding, fixed identity, reconnect/backoff, pause and site controls retained |
| Local agent protocol | Wire version `5` | Version mismatches fail closed; not the package version |
| Native connection | Handshake `2`; effect/config schemas `1` | Matching extension/host/daemon required; no silent downgrade |
| `magicvault-core` | `0.1.3`, unchanged | Existing embedded custody/API/vault format/key identity retained |
| `magicvault-primitives` | `0.1.1`, unchanged | Existing utility contract retained |
| MagicRun `tool-runtime-core` | Public Git dependency `0.1.74` | Public coordinator unchanged; non-jailed macOS launch uses descriptor-bound native spawn; exact source in Cargo.lock |
| `magicvault-test-support` | `0.4.0`, `publish = false` | Test-only, not a production surface |

The preceding source version was `0.8.3`, using local agent wire `4`. The new
closed prompt-and-fill request advances wire to `5`; mismatched clients and
daemons fail closed. Upgrade the standalone bundle together. Protocol/effect
minor versions mark the public type addition and dependency change. Native
bridge schemas, registry/vault formats, key identity, extension, core, primitives
and the pinned MagicRun source are unchanged. Custom `HumanInteraction` hosts
compile with the new default method but must explicitly implement `secret_once`
to support one-time collection; the default returns `unavailable`.

The retained discovery narrowing was introduced by `0.7.0`:
The new optional discovery fields preserve unfiltered JSON requests; Rust callers
use `BrowserTargetsQuery` instead of `BrowserQuery` for discovery (the latter
still identifies disconnects). Protocol/effect minor versions mark those public
type/enum changes. Existing custom adapters retain the default filtering method;
adapters that can exceed their inspection budget should implement early narrowing.
Filtered native commands require the updated extension/host/daemon: an old
worker refuses them, never silently falls back to broader delivery. Upgrade the
matching bundle and verify the loaded extension version before using narrowing.
The consent change adds no model-facing grant tool. Browser permissions,
destination profiles, encrypted vault formats and key identities are preserved.
No credential gains a recipient merely because software is installed. Only a
native Always allow decision grants repeated exact-use authority.

Shared libraries version independently. Magician does not acquire CLI, MCP,
daemon, extension, HTTP or MagicRun adapter dependencies by importing custody
core. No Magician dependency update is required solely for this standalone
release. Compare actual shared source/API/format/dependency changes when advancing
a consumer's reviewed Git revision; do not freeze a separate core fork.
The separate Magician JIT source integration explicitly adds the shared browser
adapter and private HITL. Its reviewed pins and rollout boundary are documented
under [embedded consumers](integrations.md#existing-embedded-consumers).

## Installing and upgrading

Use [prebuilt installation and lifecycle](distribution.md), [source build and setup](setup.md) and the
[extension installation instructions](browser-usage.md#chromium-extension).
Local npm tarballs are supported; no registry publication, Apple-verified binary
release or store listing is asserted.
`package-extension` creates unpacked assets; it does not publish or install them.

For managed installations, update matching npm packages and explicitly run
`magicvault upgrade`; npm itself never replaces a daemon. For manual installations,
stop the daemon, use matching 0.9.x CLI/MCP/native-host binaries and extension 0.6.x,
then restart and rediscover fresh handles. Run `magicvault setup` (or source
`extension install`) to select the fixed bundled identity. The old path-derived
extension may need removal/reloading and explicit restoration of grants/blocks;
subsequent fixed-ID upgrades preserve them. `upgrade` preserves a custom selected ID.
The new extension adds `alarms` and stores a profile capability plus pause/backoff
state in trusted local storage, never enrolled credentials or fill payloads.
All-HTTPS access requires explicit approval; upgrades never request it automatically.
Native-host definitions are OS-user-wide, not Chrome-profile
resources. Never point the standalone daemon at an embedded consumer's root.

Delivery profiles persist in the standalone registry; effect jobs and their
replay tombstones do not survive restart. Missing status is not evidence that
nothing ran. Old standalone binaries may reject registry fields they do not
understand. Do not strip profile/permission data to force a downgrade; restore
only a deliberately reconciled, consistent backup. Core vault framing and
instance/key identity are unchanged. [Upgrade and recovery](setup.md#upgrade-and-recovery).

The public MagicRun dependency does not require a private-fetch environment
flag. The manifest declares the compatible version without freezing development;
Cargo.lock pins the reviewed source for this build. Updating that dependency is
an explicit compatibility/test/architecture-review action, not an automatic
production change or a requirement to modify MagicRun's runtime.

## Maintaining release records

- Keep component manifests, local requirements, Cargo.lock, source versions and
  architecture fingerprints consistent. Do not bump unchanged shared libraries
  or extension assets merely to match the standalone product number.
- Keep technical usage/security docs honest about destination trust, receipt-only
  results, unsupported stateful services and native acceptance gaps.
- Record actual commands, source scope and environment in qualification records.
  Passing synthetic tests does not qualify native prompts/keychain, all installed
  browser configurations, consumer runtimes or a minimum compiler. Label local
  latency observations separately from native-UI and production performance.
- Refresh the [architecture baseline](architecture.md) only after reviewing its
  changed boundaries. A fingerprint match is not a security attestation.
- Keep profiles, capabilities, traces, private machine configuration and live
  credentials out of commits. Examples contain synthetic references only.
- Tags, pushes, registry publication, binary/installer distribution and extension
  store submissions require a separate release decision. A source version or
  local commit performs none of them.
