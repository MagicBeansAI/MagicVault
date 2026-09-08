# Fresh 0.8.3 distribution qualification — 2026-09-08

Later bounded trials and test-only observations are in the
[browser-command follow-up](results-browser-command-2026-09-08.md). This record
preserves the original candidate qualification and its immediate follow-up.

## Identity and scope

Candidate and original local test drivers: MagicVault
`81fe8c61246da33597aedb4c29535080709af3be`. The lockfile selects MagicRun
`af348ab566cbf495f59d155a328bf2cac6afa09d` (`0.1.74`). CLI/MCP/service are
`0.8.3`, effect and extension `0.6.1`, custody core `0.1.3`.
No runtime, custody, protocol, dependency or Magician change is made by this
qualification. The diagnostic follow-up below changes only test diagnostics.
The earlier [200-trial native-spawn result](results-native-spawn-2026-09-08.md)
remains separate evidence, not a substitute for testing these distribution bytes.

The manual workflows use no publisher or signing credentials. Local builds,
downloads and fresh browser profiles use SSD1; disposable runtime applications
use the internal temporary volume. Synthetic keys and deterministic test-only
consent do not qualify real Keychain, native dialogs, website permission UI or
a production standalone daemon. Personal profiles, managed native-host
definitions, LaunchAgents and the live acceptance installation are out of scope.

## Cross-platform and distribution CI

Both workflows were dispatched once on the exact source above:

- [Manual qualification 34200673459](https://github.com/MagicBeansAI/MagicVault/actions/runs/34200673459),
  attempt 1: **PASS**, macOS and Linux; `07:42:23Z–07:50:47Z`.
- [Unsigned distribution 34200546887](https://github.com/MagicBeansAI/MagicVault/actions/runs/34200546887),
  attempt 1: **PASS**, `07:40:51Z–07:51:40Z`, including artifact upload.

| Job | Host / runner image | Default Rust suite |
| --- | --- | --- |
| Linux `101978496498` | Ubuntu `24.04.4` x86_64 / `20260831.293.1` | 255 passed, 12 explicit opt-ins ignored |
| macOS `101978496290` | macOS `26.6.2` arm64 (`25G83`) / `20260831.0337.3` | 256 passed, 13 explicit opt-ins ignored |
| Distribution `101978094861` | macOS `15.7.9` arm64 (`24G830`) / `20260829.0321.1` | 256 passed, 13 explicit opt-ins ignored |

All jobs use Rust `1.92.0` and Node `22.23.2`. Each passed 87 extension/package
JavaScript tests, five qualification fixtures, 12 build-path tests and 12
architecture tests, plus full all-target compilation, the 74-input architecture
baseline and durability-adoption check. Ignored cases are not counted as passes.
Both platform jobs separately passed release-mode capacity/shutdown tests:
32 deliveries per destination, retained-job refusal, 2,000 paced status requests
from four clients, 16-connection admission/recovery, unfinished-frame drain,
and in-flight process-tree/HTTP shutdown without replay.

The distribution job additionally passed the existing repeated/child-churn
launch probes, clean standalone release build, three diagnostic registry units,
five real adapter cases and explicit instrumented-release refusal. All 20 fresh
installed CLI/native-host/MCP trials passed (13 cases per round), including the
original concurrent process/HTTP pair. Every process snapshot confirmed native
spawn, one owned child, normal successful reap and completed settlement. The
known post-exit group-kill `Denied` observation remains visible; these already
exited recipients do not substitute for the separate live-tree shutdown test.

Three installed release-mode performance cases passed: 20 latency samples per
destination, 32 capacity samples per destination plus IPC saturation/recovery,
and in-flight process-tree/HTTP shutdown. No failed trial, retry, deadline change
or concurrency reduction was needed. App-only installation/upgrade, stable
executables after npm removal and recoverable app-only uninstall passed. Only
the synthetic broker/driver was instrumented; uploaded clients are uninstrumented.

## Independently downloaded artifact

Artifact `10045954485`, `MagicVault-UNSIGNED-darwin-arm64`, was downloaded from
the successful distribution run. Its measured ZIP SHA-256 matches GitHub's
recorded digest:
`ac2abe18b189915da83260025b74ffd2dfff97bf2c6ef77b0d3d06c09428e390`.
ZIP and tar entries were inspected before extraction into fresh directories.

| Tarball | Independently measured SHA-256 |
| --- | --- |
| `magicvault-local-magicvault-0.8.3.tgz` | `327f50987aed4032a0a747879a100d53801af02679c733d1cad9ca5fa7672401` |
| `magicvault-local-magicvault-darwin-arm64-0.8.3.tgz` | `82b6942c54c020bd270ce3109351a554fbb7fa24ed560f9d3ba545b8afbb5798` |

All 14 bundle entries matched their manifest length, SHA-256 and executable bit.
The manifest had exactly the expected allowlisted entries. Every packaged source
asset and launcher matched the reviewed source, including extension `0.6.1`.
Both package versions/names were checked, with no npm lifecycle scripts. This
establishes the bytes used below, not signer identity or registry provenance.

## Local real-browser source conformance

**PASS:** macOS `26.6` arm64 (`25G72`), Rust `1.92.0`, Node `22.19.0`, Chrome
`152.0.7977.82`. The five explicitly selected CDP tests passed in 3.65 seconds:
headed and modern headless fill/original-peer continuation, preflight refusal
and partial delivery, stale navigation binding, and same-origin/opaque-frame
handling. These exercise the source-built effect adapter, not installed package
binaries. All browsers and fixture servers are disposable owned children.

```bash
CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Volumes/SSD1/magicvault/builds \
  MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  MAGICVAULT_BROWSER_TMPDIR="$candidate_dir" \
  cargo test --locked -p magicvault-effect --test chromium \
  -- --ignored --nocapture --test-threads=1
```

`candidate_dir` denotes a newly created private artifact directory, never a live
vault or personal browser profile.

## First independent installed trial — failure retained

The original package qualifier selected one round, actual installed clients,
real browsers, a 120-second idle observation and the three performance cases.
Its repacked tarballs were byte-for-byte identical to both downloaded originals.
Offline install, app-only setup/upgrade and integrity checks passed. All 13
default installed CLI/process/HTTP/native-host/MCP cases passed, followed by
installed CLI → real Chrome CDP fill/denial (3.90 seconds).

**FAIL:** the extension case ended after 11.79 seconds with
`test browser command timed out`, at the existing ten-second CDP command bound.
Neither profile's successful startup summary had appeared. The original
diagnostic does not identify the command or establish whether native-host launch
had begun. This is not evidence of another process-delivery failure, a specific
OS policy denial, or the historical external-volume loader stall.

The qualifier stopped immediately: no idle observation, local performance lane,
success summary, npm removal or app-only uninstall followed. Its isolated
application remains as evidence; fixture-owned browsers/servers are scoped for
teardown on panic. No live installation or native consent was involved.

### Diagnostic follow-up

Static review found an observability gap: setup checkpoints were printed only
after the complete connection, and all browser-command failures shared a generic
message. The test helper now reports a closed, allowlisted command label and
four fixed setup checkpoints. Unknown methods report `Other`; expressions,
parameters, responses, paths and capabilities are not printed. Three new tests
passed, including an actual local WebSocket error carrying synthetic material
and a silent peer exercising the unchanged ten-second timeout. Neither payloads
nor unknown method strings may appear in the diagnostic. Eighteen packaging/
orchestration and 12 architecture regressions passed; the 74-input product
architecture baseline still matches. No package bytes, product code, command/connection
deadline, retry policy, permission or consent behavior changed.
Final local `cargo check --locked --workspace --all-targets` passed with four
build jobs on SSD1 (8.97 seconds). Diff whitespace and 111 local documentation
links also passed validation. These follow-up checks are not a new CI run or
qualification of different package bytes.

**PASS for this individual diagnostic trial; original cause unresolved.** One
fresh trial ran against the same downloaded packages with these uncommitted
test-only diagnostics on top of `81fe8c6`. Repacked tarballs again matched both
originals exactly. The first failure remains a failure, not a flaky-test retry
discarded in favor of green. No third trial or broad process/OS-log investigation
followed, and no timing or safety policy was relaxed.

All 13 installed client cases passed, followed by installed CLI → real Chrome
(1.21 seconds). Both extension profiles completed every setup checkpoint and
connected in `810.401 ms` and `371.295 ms`. The existing full extension case
passed in 123.36 seconds: distinct profiles/handles, 65 unrelated-tab discovery,
20 fills, denial, navigation/site blocking during pending synthetic consent,
independent pause/resume and remembered reconnect. The fixture adds only its
documented loopback pregrant to a copy of the installed extension; this is not
genuine browser permission acceptance.

Startup checkpoint logging is additional diagnostic overhead. These observations
do not establish a before/after performance improvement or explain the original
timeout.

The 120-second idle observation passed with both profiles connected, no new
native-host launches and no new synthetic confirmations. It recorded 121 samples
over 120,003 ms, `3.073485` matched CPU seconds, five new and 11 departed process
identities, and no missing observations or counter regressions. Physical-footprint
sum was `2,907,511 → 2,658,098 KiB`; the sampled peak was `2,907,511 KiB`.
Process population peaked at 89 and ended at 83. These are the two complete
sampled Chrome workloads, including the 65 unrelated tabs and observer overhead,
not MagicVault-only overhead. RSS sums can double-count shared pages; transient
unsampled work is omitted. Native hosts/MCP/broker/standalone daemon are excluded.
See the [resource method and limits](extension-transport.md#browser-process-resources).

All three local installed delivery/performance cases then passed in 40.63 seconds:
32 deliveries per destination plus retained-capacity refusal, 2,000 paced status
requests from four clients, 16-connection admission/recovery, unfinished-frame
drain, 20 latency samples per destination, and in-flight process-tree/HTTP stop
without replay. Representative measurements from this trial:

| Measurement | Median | p95 | Maximum |
| --- | --- | --- | --- |
| Extension fill, 20 samples | 23.487 ms | 36.727 ms | 41.297 ms |
| Installed CLI process, 20 latency samples | 265.212 ms | 290.156 ms | 291.288 ms |
| Installed CLI HTTP, 20 latency samples | 188.374 ms | 191.164 ms | 191.215 ms |

Extension denial took 10.140 ms. In-flight shutdown measured 225.259 ms for
the owned process tree and 6.041 ms for HTTP, with recipients stopped and no
replay. The status load took 6.014 seconds; unfinished-frame shutdown took
4.970 seconds within its existing five-second framing bound. Synthetic
broker-plus-driver resident memory was `19,968 → 20,224 KiB` across that load,
with a `0.345126` CPU-second delta. These are bounded fixture observations, not
native-human latency, Internet throughput, a before/after benchmark, installed
standalone-daemon resource evidence or production performance guarantees.

After success, npm removal left the stable native executable usable, and
app-only uninstall recoverably archived only this trial's application. The
qualifier verified no vault was created and its synthetic npm home stayed empty;
keychain and service state were untouched. The first failed trial's separate app
and local logs remain retained, never overwritten by the passing trial.
An independent post-run check verified all 14 files in each of the successful
trial's two archived installed versions against the downloaded bundle's hashes,
lengths and executable bits. This disposable app's active path no longer exists; its recoverable
archive remains available locally.

The invocation below describes each fresh trial; the second used a different
new `--work` path. Build-only precompilation kept compilation out of browser
execution deadlines. No diagnostic stack sampling was enabled.

```bash
CARGO_BUILD_JOBS=4 CARGO_TARGET_DIR=/Volumes/SSD1/magicvault/builds \
  MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  MAGICVAULT_BROWSER_TMPDIR="$candidate_dir" \
  node scripts/qualify-package.mjs --packages "$candidate_dir/packages" \
  --work "$candidate_dir/fresh-trial" --app-parent /private/tmp \
  --with-rust-tests --with-browser-tests --browser-idle-secs 120 \
  --with-performance-tests
```

## Outcome and remaining gates

Fresh distribution identity, full supported-target conformance, installed
CLI/MCP/process/HTTP, actual CDP and bounded delivery/shutdown evidence are now
available for `0.8.3`. The diagnostic extension/resource trial passed, but
**extension startup reliability remains open** because the original command
timeout is neither attributed nor fixed. The next useful browser investigation
is bounded first-failure startup qualification with the new closed checkpoints;
do not substitute unchanged reruns, longer deadlines or OS permission bypasses.

Real final-artifact keychain/consent, remaining native permission/recovery and
installation lifecycle, installed daemon/native-host resources, long soak,
quarantined signed downloads, npm publication/provenance and a real agent-client
session remain separate gates in the [release matrix](release.md). No real credentials,
signing identity, publication authority or personal browser setup were used.
