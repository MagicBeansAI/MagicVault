# Owned-child signal investigation — 2026-09-08

The [reproduced exact-path failure](results-exact-path-2026-09-08.md) remains an
open release gate. This follow-up captures the missing process evidence; it is
not a runtime correction or a release qualification claim.

Later evidence: the [focused run on `58592ee`](results-focused-candidate-2026-09-08.md#first-ci-failure-retained)
reproduced a failure and observed `SIGKILL` before cleanup. Its sender/OS reason
remains unknown. The successful run below remains historical, separate evidence.

## Boundary and revisions

MagicVault changes build on `f548dbc`. MagicRun revision
`25f1c4490a5c88784598531a2d6d14f9a06879e8` adds an opt-in debug-only observer.
MagicVault's lock selects that revision without changing crate versions or other
dependencies. Installed candidate clients remain uninstrumented; only the
synthetic test broker and its runtime dependency enable both compiler cfgs.

The observer keeps fixed categories and bounded counters: owned-child wait
identity/notification checks, wait event and signal class, final reaped signal,
group ownership, before-reap versus explicit termination cleanup, and group
signal-attempt results. It keeps no PID, argument, path, environment, credential,
stream bytes or raw exit code. A thread-affine capture surrounds only one
registered blocking invocation; neither other threads nor later work inherit it.
Both capture layers have a 16-slot bound, with no growing event history.

Normal builds compile out the observer and hooks. The standard release profile
rejects them; package qualification uses a separate diagnostic target directory
and checks that refusal. Wait, signal and cleanup decisions are unchanged. No
uncertain delivery is automatically retried.

MagicRun's literal governed source bytes change even though normal behavior does
not. A future consumer upgrade must review its source-attestation digest.
Magician's existing dependency selection and source remain untouched, as do
MagicVault custody, wire formats and the live `0.8.1` acceptance installation.

## Local validation

Host: macOS `26.6` arm64, Rust `1.92.0`; builds use SSD1.

- MagicRun normal batch regression suite: 17 passed; PTY suite: 8 passed.
- MagicRun all-target check and architecture gate: passed.
- MagicRun diagnostic isolation/budget units: 2 passed.
- MagicRun diagnostic real-process tests: 2 passed, covering normal exit,
  nonzero exit, self-SIGTERM, self-SIGKILL and explicit deadline cleanup.
  Wait observations and final reaped results agreed for the four exit cases.
- MagicVault diagnostic units: 3 passed; real nonzero/signal/permission/missing
  recipient classifications: 4 cases passed in one integration test.
- MagicVault normal all-target check, 14 packaging-driver tests and the reviewed
  architecture baseline: passed.
- One offline installed CLI/native-host/MCP trial: 13 cases passed (three
  separately opt-in performance tests ignored), followed by recoverable app-only
  uninstall. The standard instrumented-release build was correctly refused by
  MagicRun's explicit guard.

This local installed trial used the previously downloaded unsigned `b475f3c`
candidate identified in the exact-path record, not newly built release bytes.
The test driver used this follow-up's source and the new locked MagicRun revision.
The owned process reported an ordinary successful exit both before cleanup and
after reap, with matching wait identity and no wait error. The subsequent
group-SIGKILL attempt reported `Denied`; this is retained as an OS return
classification, not evidence of successful cleanup or the original failure's
cause. No recipient descendants were exercised by that particular case.

MagicRun's own dev-dependency tests run in its workspace; Cargo refuses to run
them as a Git dependency from MagicVault. MagicVault instead validates the
observer through its real process-adapter and exact installed-client fixtures.

## CI investigation

[Unsigned run 34187369986, attempt 1](https://github.com/MagicBeansAI/MagicVault/actions/runs/34187369986)
**PASSED** on MagicVault `01a1cfd97f8d8230c6c7ee710f0970834699dbd2` and the
locked MagicRun revision above. Job `101938310547`; macOS `15.7.9` arm64,
image `20260829.0321.1`, Rust `1.92.0`, Node `22.23.2`. No retry was requested.

| Executed lane | Result |
| --- | --- |
| Normal all-target check and release build | PASS |
| Default Rust suite | 255 passed, 13 explicitly opt-in tests ignored |
| JavaScript / Python / architecture | 88 JavaScript and 24 Python passed; 72 architecture inputs match |
| Serial and async process stress | 200 serial launches and 200 async launches with 2,000 noise children passed |
| Diagnostic isolation/classification and release refusal | 3 units, four real recipient cases and explicit release guard passed |
| Offline installed CLI/native-host/MCP | 20 independent fail-fast rounds passed; 260 default case executions |
| Separate bounded performance/capacity/shutdown lane | All 3 opt-in tests passed |
| npm removal / recoverable app-only uninstall / upload | PASS |

All 20 process snapshots showed one owned child, a matching child notification,
an ordinary successful exit before cleanup, and the same successful final reap.
They recorded 2–3 wait polls, no interruptions/errors, one before-reap group kill
attempt classified `Denied`, and no explicit termination cleanup. The denial
classification also appeared locally and does not identify the original failure's
cause. The separate in-flight child-tree test confirmed its owned recipient
stopped without replay; these are different scenarios, not interchangeable proofs.

The 20-sample CLI delivery lane measured process median/p95 `93.847/100.329 ms`
and loopback HTTP `224.948/266.147 ms`. The capacity lane used 32 operations per
surface and 2,000 status requests at concurrency four; capacity recovered.
In-flight shutdown measured `326.994 ms` for the owned process tree and `2.064 ms`
for HTTP, with both recipients stopped and no replay. These include synthetic
consent and test-driver costs, not production latency or real native acceptance.
Resource scope is the in-process synthetic broker/driver, not a standalone daemon
or browser. This run did not execute a real-browser or long-soak trial.

Uploaded unsigned artifact: `10041209598`, `MagicVault-UNSIGNED-darwin-arm64`,
6,531,459 bytes. GitHub-reported ZIP SHA-256:
`15c07b224d543d81296268b9a2e9065abda99ef50fa527570c0d4180f2a3a24f`.
It is a temporary Actions artifact, not a signed or published release; it was
not downloaded or installed on the live acceptance setup in this follow-up.

The original intermittent failure did **not** recur. Its signal/source remains
unknown: this pass validates the observer and this bounded run, not a fix. The
next informative recurrence must retain the before-cleanup and final-reap
categories; do not rerun a failure away or automatically replay an uncertain
delivery. Signing, publication and genuine native acceptance remain separate gates.
