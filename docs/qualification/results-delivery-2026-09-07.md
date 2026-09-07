# Browser, process and HTTP qualification — 2026-09-07

Source: **MagicVault 0.4.0 alpha**, working changes based on
`5f1b52a60bea61264a61c609dcc997944767bcf8`, before a release commit.
The [architecture baseline](../architecture-baseline.json) records the reviewed
production source/dependency fingerprints. No commit, push, new tag, publication
or CI execution is attested by this record.

## Scope and environment

- macOS 26.6 (25G72), arm64; Rust/Cargo 1.92.0; Node.js 22.19.0;
  Google Chrome 152.0.7977.82. Minimum compiler, other OSes and browser brands
  were not independently qualified.
- Cargo artifacts: `/Volumes/SSD1/magicvault/builds` and separately
  `/Volumes/SSD1/magicrun/builds`. Cargo build concurrency was four jobs.
  Dependency resolution downloaded the required public packages; tests used
  the committed lockfile. No private dependency-fetch flag was required.
- Shared `magicvault-core 0.1.3`, `magicvault-primitives 0.1.1`, extension assets
  and native wire 1 are unchanged. Standalone crates use 0.4.0 / agent wire 3.
- The process adapter uses public MagicRun `tool-runtime-core 0.1.73` at
  `943ccd2a497880fd34c0ea6bb8713de8bdd55a8c`. MagicRun runtime/API/manifests/lockfile
  are unchanged; only its README, architecture description and document
  fingerprint were updated. No Magician files were changed or tests run.
- Test custody uses synthetic keys and human providers in disposable roots.
  HTTP recipients are local fixtures, including real TLS with a test-only root
  and address pin. CLI/MCP tests run the actual shipped subprocesses and IPC.
  Browser tests own fresh temporary Chrome profiles and children.

## Completed executions

| Lane | Result | Scope |
| --- | --- | --- |
| MagicVault `make check` | PASS | Architecture and durability guards; workspace/all-target compilation |
| MagicVault `cargo test --locked --workspace` | **199 passed**, seven opt-in cases ignored | Full default Rust suite, including core compatibility and new process/HTTP/CLI/MCP cases |
| Node extension and qualification-fixture tests | **17 passed** | 12 extension VM/DOM cases plus five real local recipient contracts |
| MagicVault `make test-build-paths test-architecture` | **23 passed** | 12 artifact-routing and 11 architecture-gate regressions |
| `make test-browser-native` | **Five passed** | Headed/headless delivery, original-tool continuation, controls/partial writes, navigation and frames |
| `make test-cli-native` | **One passed** | Actual CLI → daemon → real Chrome, synthetic custody/consent |
| `make build-standalone` | PASS | Optimized CLI/daemon, MCP and native-host executables; Cargo reported 1m 16s including dependency compilation |
| Release executable version/help | PASS | CLI and MCP report 0.4.0; CLI advertises the new profile/delivery/status commands; no vault initialized |
| Release CLI `cli_flow` / `delivery_cli` and MCP `end_to_end` | **10 passed** | Optimized real client/IPC/effect flows, audit-failure reconciliation and compile-time dependency payload-log suppression; overlap the default cases |
| MagicRun `make check build` | PASS | Architecture/durability guards, all-target compilation and workspace build |
| MagicRun `cargo test --locked --workspace` | **486 passed** | Full Rust suite at the unchanged runtime source |
| MagicRun `make test-build-paths test-architecture` | **23 passed** | Routing and architecture tooling |

**Distinct cases: MagicVault 245; MagicRun 509.** MagicVault's six explicitly
enabled browser cases are counted once, not also as default-suite passes.
The one public-site browser case remains ignored and was **not rerun** for this
source; [older browser evidence](results-2026-09-07.md) is historical, not a new
public-provider qualification. MagicRun's nested child-harness invocation repeats
an existing test and is not counted as another case. Targeted reruns overlap the
workspace suite and do not increase these totals.

Local relative documentation links were checked across 19 Markdown files; both
reference-only profile JSON examples parsed. Git whitespace checks passed in
both repositories. These checks do not validate remote URLs or certify usability.

The final warm MagicVault test compilation took 2.27 seconds; individual Rust
test binaries completed in at most 1.78 seconds. MagicRun's main 461-case library
binary took 15.46 seconds. These are test-run observations, **not effect latency,
throughput, idle-resource or production performance benchmarks**.

## Covered boundaries

- Reference-only closed schemas and exact caller-owned destinations; no model
  command, URL, input, approval or raw-output override.
- Process env/stdin delivery through MagicRun, changed executable refusal,
  loader-variable refusal, bounded cancellation/flood and cleanup when an
  embedded async caller drops its future.
- HTTP verbs and context encoding, loopback/public-address fences, native TLS
  verification behavior with valid/untrusted/wrong-host synthetic certificates,
  withheld raw/encoded echoes, redirects/lost replies without replay, response
  caps, stalled body deadlines and invalid header refusal before dispatch.
- Broker denial/cancel/removal/revocation/shutdown, registry capacity, profile
  persistence, spent-ID retention, old-epoch refusal and typed durable audit.
- Post-recipient audit failure faults custody but leaves status reconcilable
  through actual CLI IPC. MCP's actual SDK subprocess invokes browser, process
  and HTTP adapters, with a closed administration-free catalog.
- Existing browser behavior, shared custody formats and core compatibility
  fixtures remain covered. This is not a substitute for Magician-owned tests.

An initial JS fixture run was denied loopback binding by the execution sandbox;
the same suite passed with the required local-socket permission. Tooling checks
correctly refused stale architecture fingerprints during implementation. The
durability ratchet also required explicit review of one new **test-only** rename
that preserves a synthetic journal before injecting an audit-write failure.
Its annotated baseline retains the three existing production counts; no
production durability boundary was weakened to make the checks pass.

## Remaining gates and cleanup

Installed extension/native messaging, genuine human dialogs/keychain, native
maximum-size profile rendering, broader platforms, real provider integrations,
measured performance, sustained load and crash/power-loss recovery were **not
qualified** here. No coverage-percentage measurement or security audit is claimed.
Use the [delivery](process.md), [CLI](cli.md) and [extension](extension.md) runbooks.

Already-running service/PID/PTY refresh, arbitrary credential files, private HTTPS,
response-content return and filtering other tools' observations remain outside
the implemented product contract. Receipt-only delivery is not a sandbox or a
promise that authorized recipients cannot copy credentials.

Test-owned children/listeners and temporary roots are cleaned by their fixtures.
No live vault, keychain identity, native-host definition, LaunchAgent, personal
browser profile or credential was installed, adopted, removed or modified.
Build caches remain on SSD1. Documentation-only changes after these executions
do not alter the runtime source qualified above.
