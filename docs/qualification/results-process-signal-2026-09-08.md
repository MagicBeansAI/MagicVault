# Owned-child signal investigation — 2026-09-08

The [reproduced exact-path failure](results-exact-path-2026-09-08.md) remains an
open release gate. This follow-up captures the missing process evidence; it is
not a runtime correction or a release qualification claim.

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

The next unsigned candidate run will perform at most 20 independent installed
trials, stop at the first failure and retain its closed observations. A passing
run cannot establish the original intermittent failure's cause or fix it.
Signing, publication and genuine native acceptance remain separate gates.
