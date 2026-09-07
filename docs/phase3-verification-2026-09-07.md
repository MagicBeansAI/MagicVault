# Phase 3 verification — 2026-09-07

Historical initial build/default-suite record. Subsequent real Chrome and CLI
qualification is recorded separately in the
[real-world results](qualification/results-2026-09-07.md). Statements about
unexecuted browser cases below describe this earlier run, not the later evidence.

Result: workspace/all-target compilation, optimized standalone builds, extension
source packaging, targeted tests, and a subsequently authorized full workspace
test run passed. Real-browser/native release qualification remains outstanding.

## Source and environment

- Phase 3 `0.3.0` working tree based on
  `baba4b1b68fa4f2771a5436175360664fc7a2470`, with uncommitted implementation changes.
  This report does not attest a published tag or unchanged future revision.
- macOS 26.6 (25G72), arm64; Rust/Cargo 1.92.0; Node.js 22.19.0.
- Locked dependency resolution, offline Cargo operation, external temporary
  build cache. No dependency versions changed during verification.
- Core `0.1.3` and primitives `0.1.1` source remain unchanged. No Magician or
  MagicRun changes, commits, pushes, CI dispatch, or live installation occurred.

## Checks and builds

| Command / scope | Result |
| --- | --- |
| `make check` — durability ratchet and `cargo check --locked --workspace --all-targets` | Passed |
| `make build-standalone` — optimized CLI/daemon, MCP, native host | Passed; all three arm64 executables produced |
| Release CLI/MCP `--help` startup | Passed without opening a store |
| `make package-extension` | Passed; assembled source assets in `dist/extension` |
| `node --check` on packaged worker, options and fixed fill function | Passed |

The initially failing durability ratchet identified the browser test's direct
journal rename. It deliberately preserves a synthetic journal and obstructs its
old path to inject a post-effect audit failure; it is not atomic publication.
The test now explains this and the baseline explicitly counts that one reviewed
test exception. All three inherited production counts remain unchanged; the
scanner and its two-way regression enforcement were not bypassed or weakened.

## Initial targeted tests

The executed scope was `make test-compatibility`, `make test-foundation` and
`make test-browser`. The foundation lane's six retained unit cases were restored
and executed separately using these exact filters:

```sh
cargo test --locked -p magicvault-service --lib broker::tests
cargo test --locked -p magicvault-service --lib storage::initialization_tests
cargo test --locked -p magicvault-mcp --lib transport::tests
```

All commands used the same offline, external-cache configuration. CLI/MCP cases
shared by the lanes and reruns are counted once below.

| Selected scope | Distinct tests passed |
| --- | ---: |
| Core storage/audit wire, metadata projection, audit-only unit cases | 7 |
| Shared primitive unit/integration cases | 24 |
| Foundation protocol | 2 |
| Foundation service integration and retained broker/storage unit cases | 18 |
| CLI foundation/browser flow and output/logging safeguards | 4 |
| SDK MCP integration/catalog/output and bounded transport encoder | 5 |
| Browser protocol | 3 |
| CDP synthetic transport | 5 |
| Native bridge transport | 3 |
| Browser authorization/jobs/audit/lifecycle | 9 |
| Native-host identity/configuration/definition unit cases | 2 |
| Actual native-host subprocess and authenticated daemon bridge | 2 |
| Extension fixed-function and worker JavaScript | 12 |
| Total | **96** |

This is **84 Rust tests and 12 JavaScript tests**, all passing in the final
executions. Additionally, these two existing safeguards passed in release mode
(not counted again in the distinct total):

```sh
cargo test --locked --release -p magicvault --test cli_flow dependency_payload_logging_is_compiled_out_of_shipped_binaries
cargo test --locked --release -p magicvault-mcp --test end_to_end dependency_payload_logging_is_compiled_out_of_shipped_mcp
```

The initial restricted environment denied local socket binding. Two IPC cases
timed out waiting for their listeners and four CDP cases received an OS permission
error. A minimal loopback probe confirmed the restriction. The affected targeted
lanes passed with temporary local socket permission, without changing production
behavior or replacing the real transport fixtures with mocks at the client edge.

Splitting the targeted lanes had also omitted three retained broker unit cases,
two initialization cases and one MCP encoder case. All six now pass and are
explicitly retained in `test-foundation` for future runs.

## Subsequent full workspace run

After separate authorization, `make test` (`cargo test --locked --workspace`)
passed in **22.88 seconds wall time**, including 6.08 seconds of compilation with
the warm external cache. It exercised **171 Rust tests**, with zero failures and
two explicitly ignored real-browser qualification cases. Unit, integration and
documentation-test targets completed successfully; no executable doctest cases
were present. This was the default workspace configuration, not an all-features
or cross-platform qualification matrix.

The complete extension JavaScript suite also ran again with
`node --test extension/tests/fill.test.cjs extension/tests/worker.test.cjs`:
**12 passed**, zero failures, **0.07 seconds wall time**. Both commands were timed
with `/usr/bin/time -p` and ran concurrently. Total distinct passing coverage for
this full run is therefore **183 tests**; earlier targeted/release reruns must not
be added again to that total.

The same environment and locked offline dependencies were used. Temporary local
socket permission was granted for synthetic transports; ignored tests were not
enabled. No code fixes were needed for this full-suite run.

## What this does not establish

No real Chrome/Chromium session, native consent dialog,
keychain operation, installed extension/native-host workflow, live vault, provider,
consumer application, crash/power-loss drill, benchmark or coverage-percentage
measurement ran. CDP and extension behavior used synthetic peers/DOM objects;
actual CLI/MCP/native-host subprocesses and daemon IPC did run.

The ignored real-browser fixture sources compiled through the all-target check,
but were not executed. The published minimum compiler version and other operating
systems were not independently qualified in this run. Actual browser/native
acceptance and performance/recovery gates remain in [testing.md](testing.md).
