# Completion/admission ordering — 2026-09-08

Source candidate: standalone CLI/MCP/service `0.8.2`. No publication, live upgrade
or signing is implied. Shared custody, primitives, MagicRun, protocol/effect versions,
extension and wire formats are unchanged. The production correction is entirely
inside the standalone service's completion ownership.

Corrected source revision: `23e648ccfbed191bc4090955b04a0e73552dd99a`.

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

## Corrected-revision local qualification

The local offline npm qualifier passed ten independent, fail-fast rounds using
an explicitly selected fresh internal temporary application and SSD1 build/package/
browser artifacts. Each round ran 13 installed CLI/MCP/native-host cases and one
two-profile real extension test: 130 client cases, 20 fresh starts and 200 fills.
Startup samples were 318.941–863.800 ms, below the unchanged 15-second pass bound.
Denial, document/navigation refusal, independent profiles and pause/reconnect
assertions passed. The subsequent recoverable app uninstall and npm removal passed.
No personal keychain, OS service, browser profile or live acceptance app was changed.

Five real headed/headless CDP/control/frame/navigation cases and the shipped-CLI
real-browser case also passed. Three explicit installed-client performance/shutdown
tests passed. The 32-delivery batch observed process median/p95 249.092/306.145 ms
and HTTP 194.409/202.735 ms; capacity refused a 33rd new job without dispatch while
reconciliation preserved exactly-once invocation. A 2,000-status/four-client load
completed in 6.006 s. Test-process idle CPU was 295 µs over two seconds; loaded CPU
was 404.845 ms. RSS was 19,872 KiB after idle and 20,176 KiB after load.

The 16-frame IPC saturation/recovery test passed. Incomplete-frame shutdown took
4.971 s within existing framing/drain bounds; actual process-tree shutdown took
224.347 ms and in-flight HTTP shutdown 5.029 ms, with recipient work stopped and
no replay. These are **synthetic in-process broker/test-driver** counters, not an
installed-daemon/browser-tree production benchmark. Local CDP verification overlapped
part of the performance lane; these observations do not establish isolated throughput
or close the long-soak/resource release gate.

Local unsigned tarball SHA-256 (not the CI artifact): launcher
`42b2656f10a6be0af7908f55090198d223fa302993dd4441d016b94ca19bb65b`;
native `c9446ec6d66864f85b148a476144de81e4e7450a80ee1e5cb2fba3fc3434645a`.

## Corrected-revision CI

The first [manual qualification at `23e648c`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34179210734)
passed both Ubuntu and macOS. Each lane passed 254 default Rust tests and two
explicit resource/shutdown tests, all 85 JavaScript tests, 24 build-path/architecture
tests and all-target compilation. The previously failing consent cases, deterministic
completion tests and immediate-restart case passed. This is new-revision evidence,
not a rerun of the failed pre-fix workflow.

The first [unsigned distribution run at `23e648c`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34179148138)
also passed: full checks/tests/build, the 200-process probe, 20 independent installed
CLI/MCP/native-host rounds and all three explicit performance/shutdown probes.
Artifact `10038459641` contains only the two expected `0.8.2` npm tarballs. Its
6,530,093-byte ZIP SHA-256 is
`7fdfbdff54f60cc852b85ee8bd215535ff9e9c9171ea399e098cb522ae579a1b`;
the temporary Actions download expires 2026-09-22. This is not a public release.

The CI ZIP was independently downloaded to SSD1, matched that hash and had its
member names/types inspected. A fresh local qualifier then passed all 13 installed
client/native-host cases and a two-profile real-extension trial using those CI-built
packages: startup 706.600/316.348 ms, 20 measured fills, denial, profile independence
and recovery assertions. Its repacked installation inputs were byte-for-byte equal
to the downloaded tarballs. Recoverable app uninstall and npm removal passed.

CI tarball SHA-256: launcher
`42b2656f10a6be0af7908f55090198d223fa302993dd4441d016b94ca19bb65b`;
native `7683ee94781afdfdda6503ab4dd3dd955aedfba802fccca966a6c2f70baad7d6`.
Neither downloaded/local unsigned artifact trial qualifies Gatekeeper quarantine,
publisher signing/notarization, genuine native-dialog acceptance on `0.8.2`, a
live agent-model session, whole-daemon/browser-tree resources or a long soak.
The two original intermittent findings remain open despite these successful runs.
