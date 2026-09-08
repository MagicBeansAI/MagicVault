# Reliability investigation and bounded qualification — 2026-09-08

Status: **additional evidence, not closure of the two intermittent findings**.
The [subsequent policy investigation](results-startup-policy-2026-09-08.md)
narrows the sampled external-volume startup issue; process uncertainty remains open.
This record covers the `0.8.1` investigation; a subsequently demonstrated standalone
admission race and its `0.8.2` correction have a [separate record](results-completion-order-2026-09-08.md).
No production Rust behavior, extension/launcher code, custody schema, MagicRun revision or
Magician source changed. Standalone remains `0.8.1`, effect/protocol `0.6.0`,
extension `0.6.1`, core `0.1.3`, primitives `0.1.1`; agent wire 4/native handshake 2.

## Revision and isolation

Qualification-only changes are committed as
`5960395a76f3e6bdab2198ff5573c92837c6ac14`, based on `9752b12`, with
unchanged release binaries and lockfile. Local Rust 1.92.0, Node 22.19.0, macOS 26.6
arm64, Chrome 152.0.7977.82. Build/package artifacts and disposable browser profiles
used SSD1; explicitly identified comparison installations used private internal
temporary directories. Each test owns fresh synthetic custody and consent, not a
personal browser profile, native UI, keychain or LaunchAgent. The dedicated live
acceptance installation was not modified. Raw stack/policy logs stay private.

Lockfile SHA-256:
`1d4354326e498932da452c5c41a4cfb61b2956f750bd7caf2b4573a6a5048a32`.
The source and external installed native host had identical SHA-256:
`08bfc68c63a68ad07e20a3aa5f9fb4bc22ab7ff8fdad12ff4a50041d9b1fb55e`.
These are local unsigned bytes, not the prior Actions artifact or a signed release.
Qualified local tarball SHA-256: launcher
`f60786c524ec9873504d85f03b7820efd190085156767ddec2576e6820dfdb53`;
native `fcbf6424e05bb2b1e3eb3c81c57e5e700801c1de0ec36706567f78e34cabac8a`.

## Findings retained

### Packaged process uncertainty: reproduced in CI, still open

One hundred independent installed npm CLI delivery-test runs passed. Each exercised
the process path and HTTP completion/persistence-uncertainty cases; the loop stopped
on any failure. This did not reproduce the [original CI failure](results-distribution-ci-2026-09-08.md),
and does not identify or fix its cause. The closed error, dispatch uncertainty and
recipient-marker assertions remain enabled. No deadline, success assertion or
runtime uncertainty handling was weakened. CI now runs 20 independent installed
client trials to increase opportunities to capture the original failure.

The first [unsigned distribution run at `5960395`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34176656487)
**failed on trial 4**, after three complete installed rounds passed. The process
receipt was `uncertain`, closed error `unavailable`, `may_have_run: true`, with no
recipient completion marker. This occurrence did not take the adapter's timeout
or persistence-uncertainty branch. It does not prove that the child never ran or
identify its terminal cause. No candidate artifact was uploaded. Subsequent fixture
diagnostics add only interpreter-entry and material-presence booleans; no credential
or recipient stream is logged. A passing rerun will not close this finding.

A subsequent explicit adapter stress probe records terminal classes and known-error
booleans under `cfg(test)` only. Its initial 1,000-trial local configuration exceeded
the new harness's 90-second overall budget without a delivery assertion failure;
the workload was corrected to 200 trials, retaining the 90-second harness bound
and the original two-second per-process deadline. This is diagnostic coverage, not
a fix or a product performance qualification.

### Native extension startup: reproduced before the application entry point

Ten fresh source-browser trials passed (two profiles per trial). A subsequent
fresh external-volume installed package passed its 13 non-browser client/host
tests, then **failed** browser startup at 15.76 seconds: connected false,
connecting true, paused false, reason absent. Its wrapper ran once; the synthetic
confirmation count remained at the two setup calls, before browser pairing.

Two diagnostic runs retained the 15-second failure bound and observed for 45
seconds. Both remained stuck at the same stage. A one-second sample of the verified
fixture-owned host showed a single thread in `dyld` → binary mapping → `__open`,
before MagicVault initialization, with only 176 KiB physical footprint. Correlated
macOS TCC logs contained a Full Disk Access **preflight denial**, not proof that
this denial caused the loader stall or that any permission should be granted.
The sampled child was no longer present after disposable-browser cleanup.

The same package passed when installed in a fresh internal temporary directory.
The external binary then passed through its physical version path, and subsequently
through the original `current` symlink too. Therefore neither a symlink fix nor
an OS permission diagnosis is established. No production launch change, security
bypass, automatic consent or OS permission mutation was made. Historical startup
failures cannot all be attributed to this newly observed loader stage.

## Requalification and measurements

The revised driver ran ten fail-fast installed-package rounds using an internal
temporary application and external build/package/browser artifacts. All ten
passed: 130 client/host test executions, ten two-profile real extension/MCP runs,
and 200 measured fills plus the existing denial, navigation/blocking, pause and
reconnect assertions. All 20 profile startup samples were below the unchanged
15-second deadline. Three explicit performance/shutdown tests then passed, followed
by npm removal, stable native executable checks and recoverable app uninstall.

| Installed-package observation | Result |
| --- | --- |
| 32 process deliveries | PASS; median 251.447 ms, nearest-rank p95 351.162 ms |
| 32 loopback HTTP deliveries | PASS; median 192.495 ms, p95 205.512 ms |
| Retained-job capacity | New operation refused at 32; identical completed operation reconciled without another delivery |
| Paced local status load | 2,000 requests, four concurrent clients, 6.401 s |
| Idle test-process CPU | 302 µs over a 2-second idle window |
| Loaded test-process CPU | 386.458 ms during the status load |
| Test-process resident memory | 19,968 KiB after idle; 20,256 KiB after load; high-water 20,352 KiB at shutdown |
| IPC admission and recovery | 16 incomplete frames held; excess connection closed; service recovered after closure |
| Shutdown with incomplete frame | 4.968 s, within the existing framing/drain bounds; owned sockets removed |
| In-flight process-tree shutdown | 237.838 ms; descendant heartbeat stopped; no late marker |
| In-flight HTTP shutdown | 4.855 ms; recipient observed stream closure; no second request |

CPU/RSS describe the **synthetic in-process broker and test driver**, not an
installed standalone daemon. Reaped-child CPU counters include CLI/native child
execution and are reported separately by the test. These short local observations
include CLI startup, synthetic consent and polling; they do not establish
production throughput, browser-tree peak memory, native-dialog latency or a long
soak. Job retention and spent-ID bounds were not raised to improve measurements.

Full local all-target compilation, Rust tests, release build, 85 JavaScript tests
and 24 build-path/architecture tests passed. The architecture baseline now covers
both CI workflows (69 inputs); runtime versions and source hashes remain unchanged.
Five real headed/headless CDP/control/frame/navigation tests and the shipped CLI
real-browser test also passed on fresh synthetic profiles.
Repository-wide formatting inspection found pre-existing formatting drift;
only the three touched Rust test/helper files were formatted and checked.

## Remaining gates

The first [manual qualification run at `5960395`](https://github.com/MagicBeansAI/MagicVault/actions/runs/34176561247)
passed on both macOS and Ubuntu: each ran 251 default Rust tests, two explicit
resource/shutdown tests, 85 JavaScript tests and 24 build-path/architecture tests.
Unsigned distribution failed as recorded above, so this revision is **not fully
CI-qualified**.
The two reliability findings above remain open even when those lanes pass.
Installed standalone/browser-tree resource sampling, long soak, wider native
permission/recovery and lifecycle acceptance, signing/notarization, registry
publication and public release remain separate gates in the [release matrix](release.md).
