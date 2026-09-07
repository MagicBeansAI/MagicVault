# Prebuilt distribution qualification — 2026-09-07

Source: **MagicVault 0.5.0 alpha**, uncommitted working changes based on
`5a6b4d298a95df8ef3b82bb96096ece31740c62d`. The reviewed production/document
fingerprints are in [architecture-baseline.json](../architecture-baseline.json).
This record does not attest a commit, push, tag, registry publication or CI run.

## Scope and environment

- macOS 26.6 (25G72), arm64; Rust/Cargo 1.92.0; Node 22.19.0.
  Other platforms and the minimum Rust version were not independently tested.
- Cargo builds/checks/tests used `/Volumes/SSD1/magicvault/builds`, four build
  jobs. Local npm artifacts/installations used fresh disposable SSD1 directories.
- CLI/daemon/native-host, MCP and service are 0.5.0. Core 0.1.3, primitives 0.1.1,
  effect/protocol 0.4.0, extension 0.3.0, agent wire 3 and native wire 1 are unchanged.
- MagicRun files/runtime and Magician were not changed or tested for this work.
  The MagicRun Git dependency remains the exact reviewed source in Cargo.lock.
- Real CLI/MCP/native-host processes used synthetic broker/human/key providers.
  npm used offline local tarballs and private cache/config/home directories.
  No real vault, keychain item, LaunchAgent, browser definition or live credential
  was initialized, modified or removed by qualification.

## Completed executions

| Lane | Result | Evidence boundary |
| --- | --- | --- |
| `cargo check --locked --workspace --all-targets` | PASS | All targets compile |
| `cargo test --locked --workspace --quiet` | **213 passed**, seven ignored | Full default Rust suite; core compatibility and browser/process/HTTP regressions included |
| Installer/CLI lifecycle tests | **14 added cases**, included above | Private/durable copying, ownership, overlap, corruption, symlinks/FIFO, concurrency, activation, recoverable retirement, writer-drain timing and read-only doctor |
| Node extension/fixture/distribution tests | **23 passed** | 12 extension cases, five recipient fixtures, six new packaging/launcher/signing-helper denial cases |
| Build-path / architecture tooling | **24 passed** | 12 routing and 12 architecture cases |
| Architecture and durability guards | PASS | 63 reviewed source inputs; seven accounted durability patterns, including two explicit non-byte-store installer renames |
| Optimized CLI/MCP/native-host build | PASS | `cargo build --locked --release -p magicvault -p magicvault-mcp` |
| Offline packaged installation and lifecycle | PASS | Actual tarballs, no lifecycle hooks, version/doctor, app-only setup, upgrade, npm removal, stable executable survival and recoverable uninstall |
| Packaged CLI/MCP/native-host IPC qualification | **12 passed**, overlap default suite | Installed launcher/native binaries with synthetic custody; fills, process/HTTP, metadata, native bridge and closed diagnostics |
| Public documentation links / Git whitespace | PASS | Local relative targets and `git diff --check`; not remote-link or usability certification |

**260 distinct named automated cases** (213 Rust + 23 JavaScript + 24 tooling).
Packaged-install assertions are an additional successful integration workflow,
not arbitrarily counted as more named tests. Targeted/packaged reruns overlap
the default suite and do not inflate the count. No ignored browser case was run.

The final local package qualification command was equivalent to:

```bash
node scripts/package-npm.mjs --binary-dir "$CARGO_TARGET_DIR/release" \
  --output "$candidate_dir/packages" --scope @magicvault-local
node scripts/qualify-package.mjs --packages "$candidate_dir/packages" \
  --work "$candidate_dir/qualification" --with-rust-tests
```

The installed-client PATH contained Node and system utilities, not Rust. The
separate Rust test driver used the existing toolchain to host synthetic custody.
After npm uninstalled both test packages, stable app binaries still ran and
app-only uninstall retained a recoverable archive without creating a vault.

## Findings and limits

Static review added serialization between implicit extension installation and
uninstall, exact service-definition verification, writer-lease gating before
launch/replacement, bounded/no-follow/nonblocking source reads, version checks,
and explicit integrity-versus-publisher-authentication reporting.

An early test assumed macOS temporary paths were already canonical; it was
corrected to compare resolved paths. The isolated npm harness initially reused
one config file for user/global config; separate files fixed that npm rejection.
A local fixture bind was denied by the sandbox and passed with loopback permission.
The build-routing gate initially encountered the intentionally stale architecture
baseline and passed after the reviewed baseline was updated. These were not
suppressed or counted as successful runs.

**NOT RUN:** full native setup/pairing/keychain/LaunchAgent acceptance, an installed
extension/browser workflow, Apple signing/notarization/Gatekeeper verification,
npm registry installation/provenance, GitHub workflow execution, performance
benchmarks, minimum-toolchain and non-arm64 platform qualification. The signing
helper was syntax-checked and tested only on an invalid invocation that exits
before signing. No signing credentials were read or used.

The installer streams files with bounded buffers outside async executor work;
steady-state custody/effect paths are unchanged. These are design/code properties,
not measured latency/throughput or a no-regression proof for all consumers.
Follow the [native distribution acceptance runbook](distribution.md) before
claiming a verified public release. Do not infer that an unsigned local pass
authenticates the publisher or establishes production credential isolation.
