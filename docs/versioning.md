# Versions, compatibility and distribution

## Current component versions

**0.3.0 — browser-delivery source alpha, 2026-09-07.** The
[changelog](../CHANGELOG.md) describes the functionality and
[dated qualification record](qualification/results-2026-09-07.md) states what
actually ran. A public source repository is not a package registry publication,
binary release, extension store listing or production-safety certification.

| Component | Source version / contract | Compatibility responsibility |
| --- | --- | --- |
| `magicvault` CLI, daemon and native-host executable | Package `0.3.0` | Build together from the same checkout |
| `magicvault-mcp`, `magicvault-service`, `magicvault-effect`, `magicvault-protocol` | Packages `0.3.0` | Upgrade the standalone client/service/adapter set together |
| Chromium extension | Manifest `0.3.0` | Package the shared fill function and extension assets from that checkout |
| Local agent protocol | Wire version `2` | Version mismatches fail closed; not the package version |
| Native bridge | Wire version `1` | Separate authenticated native contract; not agent protocol v1 |
| `magicvault-core` | `0.1.3`, unchanged | Existing embedded custody/API/data-format contract retained |
| `magicvault-primitives` | `0.1.1`, unchanged | Existing low-level utility contract retained |
| `magicvault-test-support` | `0.3.0`, `publish = false` | Test-only; not a production integration surface |

The preceding standalone application version was `0.2.2` with protocol crate
`0.2.0` / wire version `1`. This update adds browser delivery and an incompatible
standalone protocol, hence the `0.3.0` minor version. Qualification and
documentation changes do not by themselves require a shared-library version bump.

Shared libraries have independent versions. Do not bump them simply because
standalone surfaces change: embedded consumers such as Magician do not acquire
the daemon, MCP, CLI, CDP adapter or extension through this checkpoint. MagicRun
is not a dependency of browser delivery.

## Installing and upgrading

Use [source build and standalone setup](setup.md) and the
[extension installation instructions](browser-usage.md#chromium-extension).
No `cargo install` registry workflow, downloaded installer or store listing is
asserted for this checkpoint. The local `package-extension` target creates
unpacked assets; it does not publish or install them.

Use matching standalone binaries and extension/native-host assets, stop and
restart the daemon when upgrading, and rediscover browser/target handles.
Native-host definitions are OS-user-wide; a different Chrome profile does not
isolate them. Follow [upgrade and recovery](setup.md#upgrade-and-recovery) before
changing an existing standalone installation. Never point it at an embedded
consumer's root or regenerate keys to bypass a migration failure.

Browser delivery remains deny-by-default for existing credentials: metadata
access does not become fill permission on upgrade. Core vault format and key
identity are unchanged, but the standalone registry/protocol is newer; an old
standalone binary is not guaranteed to read it.

## Maintaining release records

- Keep package manifests, local dependency requirements, `Cargo.lock` and the
  extension manifest consistent with the intended component versions. CLI/MCP
  version reporting derives from their package metadata.
- Record features, compatibility changes, known limits and actual evidence in
  the changelog and linked technical runbooks. Do not claim unexecuted tests pass.
- Keep [architecture.md](architecture.md) and its fingerprint baseline aligned
  with the reviewed source and component versions. Run `make check-architecture`
  before accepting source/dependency changes; baseline renewal requires review.
- Scope test claims to the recorded source and environment. The current 195
  passing cases do not replace the open installed-extension/native-human/keychain,
  performance, broader-platform or consumer-owned runtime gates.
- Keep browser profiles, stores, capabilities, private machine configuration,
  raw traces and live credentials out of source commits and published artifacts.
  Fixtures and examples must use public synthetic data only.
- A local commit does not publish a release. Tags, pushes, registry publication,
  installer distribution and extension-store submission are separate actions
  requiring an explicit release decision and appropriate qualification.
