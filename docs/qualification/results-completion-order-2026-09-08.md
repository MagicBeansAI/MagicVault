# Completion/admission ordering — 2026-09-08

Source candidate: standalone CLI/MCP/service `0.8.2`. No publication, live upgrade
or signing is implied. Shared custody, primitives, MagicRun, protocol/effect versions,
extension and wire formats are unchanged. The production correction is entirely
inside the standalone service's completion ownership.

## Evidence and correction

The first [manual qualification at `eab362c`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34177918839)
passed macOS but failed Ubuntu: two browser consent cases received `Busy` after
observing a terminal fill. A final blocking state transaction published the receipt,
then returned to an async worker that still owned the human-operation permit.
Another client could read the terminal result before that worker resumed and
released the permit. The same ownership pattern existed in process/HTTP delivery
and metadata decisions.

A deterministic local regression stops polling the current-thread async runtime
after the synthetic adapter returns, while the blocking completion transaction
continues. The old code failed with `terminal receipt became visible before
admission was released`. The fix moves the permit into the completion closure;
its drop now precedes state-lock release, after recipient cleanup and durable audit.
There is no retry, extra sleep, wider concurrency, early material release or new
agent diagnostic. Audit failure remains uncertainty and faults new effect admission.

Three deterministic tests cover successful/audit-uncertain browser fills, denied
delivery without a process and decided metadata. All passed after the correction.
The existing 63-test service run also passed before the last two tests were added.

Full local qualification then caught a shutdown/restart consequence: the old
slot-only drain could return while the finishing worker still held a broker/instance
lock reference, so the immediate restart test received `Busy` opening its own
fixture vault. Shutdown now cancels work, acquires admission and closes/waits for
all three tracked background-job paths, including future destructors. The tracker
does not retain completed jobs. This follow-up is part of the correction, not a
suppressed test or a delay added to the restart fixture.

After that follow-up, full local all-target compilation and 254 Rust tests passed
(12 explicitly opt-in tests ignored by the default lane), including all 65 service
unit tests and 13 foundation integration tests. All 85 JavaScript and 24 build-path/
architecture tests passed, as did the standalone release build. The reviewed
architecture baseline covers 71 inputs. Regression assertions also verify that
quiescence leaves no worker-owned broker reference in the disposable fixtures.

## Separate findings remain open

The [diagnostic unsigned run at `eab362c`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34177841055)
passed its 200-process stress probe, all 20 independent installed-client trials and
resource/shutdown tests. That is **pre-fix `0.8.1` evidence**, not `0.8.2` qualification
or an explanation of the separate `uncertain` process receipt. Artifact
`10038006343`, ZIP SHA-256
`e6f6d678adfcfe2a4593de06f212bf72be96ee4d36461e0cc6f8ed162ac63262`,
contains unsigned tarballs and is not a public release.

The process uncertainty and pre-entry-point native-host startup observations remain
open in the [reliability investigation](results-reliability-2026-09-08.md).
The admission race is a demonstrated additional defect, not a claimed cause of
either original finding. Fresh local/CI/installed-browser qualification of `0.8.2`
must be recorded below before claiming the corrected revision passes those gates.
