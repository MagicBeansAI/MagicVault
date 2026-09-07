# Versions, compatibility and distribution

## Current component versions

**0.4.0 — browser, process and HTTP delivery source alpha, 2026-09-07.**
The [changelog](../CHANGELOG.md) describes the supported functionality and
[delivery qualification](qualification/results-delivery-2026-09-07.md) records
execution evidence. A public source repository is not a registry publication,
binary release, extension store listing or production-safety certification.

| Component | Source version / contract | Compatibility responsibility |
| --- | --- | --- |
| `magicvault` CLI, daemon and native-host executable | Package `0.4.0` | Build together from the same checkout |
| `magicvault-mcp`, `magicvault-service`, `magicvault-effect`, `magicvault-protocol` | Packages `0.4.0` | Upgrade standalone clients/service/adapters together |
| Chromium extension | Manifest `0.3.0`, unchanged | Shared fill function and native wire are unchanged |
| Local agent protocol | Wire version `3` | Version mismatches fail closed; not the package version |
| Native bridge | Wire version `1`, unchanged | Separate authenticated native contract |
| `magicvault-core` | `0.1.3`, unchanged | Existing embedded custody/API/vault format/key identity retained |
| `magicvault-primitives` | `0.1.1`, unchanged | Existing utility contract retained |
| MagicRun `tool-runtime-core` | Public Git dependency `0.1.73` | Existing public coordinator; exact source recorded in Cargo.lock; runtime unchanged |
| `magicvault-test-support` | `0.4.0`, `publish = false` | Test-only, not a production surface |

The preceding standalone version was `0.3.0` with agent wire `2`.
The new wire exposes fixed destination profiles and receipt-only process/HTTP
operations. Existing browser permissions and encrypted vault formats are
preserved. New profile authority is absent by default; no credential becomes
deliverable to a new recipient merely because the daemon was upgraded.

Shared libraries version independently. Magician does not acquire CLI, MCP,
daemon, extension, HTTP or MagicRun adapter dependencies by importing custody
core. No Magician dependency update is required solely for this standalone
release. Compare actual shared source/API/format/dependency changes when advancing
a consumer's reviewed Git revision; do not freeze a separate core fork.

## Installing and upgrading

Use [source build and setup](setup.md) and the
[extension installation instructions](browser-usage.md#chromium-extension).
No registry install, downloaded installer or store listing is asserted.
`package-extension` creates unpacked assets; it does not publish or install them.

Stop the daemon, use matching 0.4.x standalone binaries, restart and deliberately
reconnect/rediscover browser handles. The extension's unchanged 0.3.0 assets
continue using native wire 1; an extension version bump is not needed for these
non-browser effects. Native-host definitions are OS-user-wide, not Chrome-profile
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
  Passing synthetic tests does not qualify native prompts/keychain, the installed
  extension, all consumer runtimes, a minimum compiler or performance.
- Refresh the [architecture baseline](architecture.md) only after reviewing its
  changed boundaries. A fingerprint match is not a security attestation.
- Keep profiles, capabilities, traces, private machine configuration and live
  credentials out of commits. Examples contain synthetic references only.
- Tags, pushes, registry publication, binary/installer distribution and extension
  store submissions require a separate release decision. A source version or
  local commit performs none of them.
