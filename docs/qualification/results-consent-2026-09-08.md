# Exact-use consent qualification — 2026-09-08

Source: uncommitted `0.8.0` implementation on
`2c7d0f5` (`docs: record committed release qualification and remaining gates`).
CLI/MCP/service are `0.8.0`, protocol/effect `0.6.0`, extension `0.6.1`, agent
wire `4`. Native handshake remains `2`; bridge/config schemas remain `1`.
Shared core `0.1.3`, primitives `0.1.1`, the locked MagicRun dependency `0.1.73`
and Magician source are unchanged. No Magician suite was run.

Environment: macOS `26.6` (`25G72`), Apple Silicon, Chrome `152.0.7977.82`,
Rust/Cargo `1.92.0`, Node `22.19.0`, npm `10.9.3`. Cargo builds and disposable
browser roots use SSD1. Private IPC fixtures use OS temporary directories.
Dependencies were resolved offline; Cargo.lock SHA-256:
`65fec7d0653d14ad1390c9e1415f61a103a806edc54ae4d6501e2a8868d9cc30`.

## Implemented boundary and static review

The native use dialog now offers Deny, Allow once and Always allow. No agent
request can create a grant. Fixed process/HTTP profiles and native extension
profiles have persistent, client-owned exact-use scopes; CDP grants expire with
the connection/restart. CLI inspection/revocation/reset is separate from the
14-tool MCP catalog. [Consent semantics and limitations](../consent.md).

Review covered scope/owner isolation, final authorization before custody
resolution, revocation and late decisions, registry/audit failure, live browser
checks, executable constraints, cancellation and output withholding. Capacity
tests exposed that remembered grants would not fit the previous aggregate state
budget: the bounded registry increased to 1.25 MiB, with at most 16 grants of
12 KiB each. The existing 256 KiB reply bound remains, including maximum escaped
selector projections. Grants are not a recipient sandbox or a secrecy guarantee
against other tools. Static review is not proof that all bugs are absent.

## Executed automated lanes

| Lane | Result | Boundary |
| --- | --- | --- |
| Full locked offline Rust workspace | **PASS**, 248 tests; 9 explicit opt-in cases ignored | New exact-scope/default/revoke/restart/late-decision/write-failure tests plus existing custody/browser/delivery/client regressions |
| All extension/script JavaScript | **PASS**, 84 tests | Worker/options/fill, distribution and synthetic recipient fixtures |
| `make check test-architecture test-build-paths` | **PASS**, all-target compilation and 24 guard tests | Reviewed `0.8.0` architecture, 68 source inputs; known durable-I/O exceptions unchanged |
| `make build-standalone` | **PASS** | Matching release CLI/MCP/native-host executables |
| `make test-browser-native` | **PASS**, 5 tests, 2.84 s | Real headed/headless CDP; synthetic custody and human provider |
| `make test-cli-native` | **PASS**, 1 test, 1.70 s | Shipped CLI/daemon/real browser; synthetic human provider |
| Initial source native-extension lane | **FAIL**, 15.67 s | Startup still connecting after 15 seconds, before discovery or credential use |
| Independent packaged native-extension lane | **FAIL**, 15.76 s | Same startup timeout; preceding 13 installed client/host tests passed |
| Four fresh source trials with value-free startup diagnostics | **PASS**, 3.57 / 3.90 / 3.41 / 3.63 s | Each uses two fresh profiles, 20 fills and existing deny/block/pause/reconnect cases |
| Fresh packaged qualification with diagnostics | **PASS**, 13 reused client/host tests plus 1 real extension test, 6.82 s browser execution | Offline npm installation, installed assets, app-only upgrade, stable binaries after npm removal, recoverable app-only uninstall |

Counts overlap and must not be added. The final package report explicitly says
`passed: true`, version `0.8.0`, `isolated_browser_tests: true`, and
`live_custody_or_services_touched: false`. Its custody, consent and loopback
permission grant are test fixtures, not native acceptance.

Final all-target compilation and 12 architecture guard tests passed after the
test-only diagnostics and comment clarification. The final build-routing run
had two 20-second subprocess timeouts in its fake-Cargo `check` and `test`
recipes; an unchanged focused rerun passed all 12 cases in 0.736 s. The reason
for those timeouts is undetermined; no deadlines were extended. Whitespace and
160 relative documentation file-link targets passed (not external URLs/anchors).

### Startup reliability remains open

Both failures reported `connected: false`, `connecting: true`, `paused: false`,
`reason: null`. The integration fixture now records only wrapper-entry counts,
synthetic confirmation counts and startup elapsed time; it never captures native
frames, capabilities or raw browser/process diagnostics. No production startup,
consent, permission or timeout behavior was weakened. Later successful trials
did not reproduce the failure, so its stage and cause remain undetermined. This
is the previously observed failure class, not a demonstrated fix or release pass.

The packaged successful trial's first/second startup observations were 3.611 s
and 0.462 s; 20 synthetic fill samples had median 25.548 ms, p95 36.527 ms,
maximum 87.852 ms. These exclude human reaction time and are not a performance,
soak or reliability guarantee.

## Local artifacts

Unsigned/ad-hoc macOS Apple Silicon local-scope candidates, not published npm
packages or a public GitHub release:

| Tarball | SHA-256 |
| --- | --- |
| `magicvault-local-magicvault-0.8.0.tgz` | `b27b7b5f0d000cb8ec288f7c28d41983b7cd239649e9571fdbc34838a83f76f9` |
| `magicvault-local-magicvault-darwin-arm64-0.8.0.tgz` | `cdf94448660b5fae38bfd24a3a75029cf501f8be17e161f36a57483c9b373888` |

The runtime candidate was built before the subsequent test-only startup
diagnostics and a comment-only capacity clarification. Neither changes its
runtime logic. The earlier `0.7.0` CI runs do not qualify this implementation;
no new CI run, signing, notarization or publication was performed here.

## Native acceptance status

Pre-upgrade read-only doctor: **PASS** for the dedicated `0.7.0` installation,
ready daemon, retained client, verified app/native-host definitions and one
connected extension profile. Its existing vault, keychain, permissions and
pairing were not replaced. [Earlier native evidence](results-discovery-2026-09-08.md).

Dedicated managed upgrade `0.7.0` → `0.8.0`: **PASS**. Matching offline npm
packages were installed with scripts disabled. The managed upgrade reported
service restart, preserved vault and retained previous bundles. Read-only doctor
then confirmed `0.8.0` integrity, ready daemon, same paired client and one
reconnected extension profile. `list-consents` returned an empty list. This
confirms the loaded worker's connection, not manual verification that Chrome
reloaded the new `0.6.1` explanatory options copy. No native approval was
automated and no keychain value was read by the operator.

Genuine `0.8.0` native consent qualification is **INCOMPLETE**:

| Case | Result | Observation |
| --- | --- | --- |
| D-01 process registration deny | **PASS** | Human confirmed clicking Deny; CLI exited with `denied`; destination and consent lists remained empty; no recipient execution marker existed |
| D-02 process registration allow | **PASS** | Human confirmed Allow; CLI returned one reference-only process profile; consent list stayed empty and no recipient marker existed |
| D-03 process use deny | **PASS** | Human confirmed Deny; same operation settled as `denied`, error `denied`, `may_have_run: false`; no recipient marker and no remembered consent |
| D-04a process Allow once delivery | **PASS** | Human confirmed Allow once; same operation completed without error; recipient recorded exactly one nonempty-credential execution; consent list remained empty |
| D-04b process asks again | **PASS** | Human confirmed Deny on the next fresh use; it settled as `denied` with `may_have_run: false`; execution count stayed at one and consent list stayed empty |
| D-05a process Always allow | **PASS** | Human confirmed Always allow; operation completed without error; recipient count increased to two; one client-owned grant bound to the exact process profile appeared |
| D-05b process remembered-use reuse | **PASS** | Separate fresh matching request completed without a further human approval; recipient count increased to three and the same exact-profile grant remained |
| D-06 process consent revocation | **PASS** | CLI acknowledged exact-grant revocation; human confirmed Deny on a fresh use, which settled as `denied` with `may_have_run: false`; execution count stayed at three and consent list stayed empty |
| Process remembered-grant persistence and remaining recovery | **NOT RUN** | Process profile survived the shared daemon restart and its revoked grant did not return, but a remembered process grant was not separately tested across restart |
| H-01 HTTP registration deny | **PASS** | Human confirmed Deny; CLI returned `denied`; no HTTP profile or consent grant appeared; recipient delivery-request and credential-received counters remained zero |
| H-02 HTTP registration allow | **PASS** | Human confirmed Allow; CLI returned one reference-only HTTP profile; consent list stayed empty and both recipient counters remained zero |
| H-03 HTTP use deny | **PASS** | Human confirmed Deny; same operation settled as `denied`, error `denied`, `may_have_run: false`; both recipient counters stayed zero and consent list stayed empty |
| H-04a HTTP Allow once delivery | **PASS** | Human confirmed Allow once; same operation completed without error; recipient counted exactly one request with a nonempty credential header; consent list stayed empty |
| H-04b HTTP asks again | **PASS** | Human confirmed Deny on the next fresh use; it settled as `denied` with `may_have_run: false`; both recipient counters stayed at one and consent list stayed empty |
| H-05a HTTP Always allow | **PASS** | Human confirmed Always allow; operation completed without error; both recipient counters increased to two; one exact HTTP-profile grant appeared |
| H-05b HTTP remembered-use reuse | **PASS** | Separate fresh matching request completed without further human approval; both recipient counters increased to three and the same grant remained |
| H-07 HTTP grant across daemon restart | **PASS** | Controlled stop/start changed daemon epoch while retaining pairing, profiles and the exact grant; counters stayed at three through restart; one fresh post-restart request completed without another approval and increased both counters to four |
| H-06 HTTP consent revocation | **PASS** | Human confirmed Deny on a fresh post-revocation use; same operation settled as `denied` with `may_have_run: false`; both recipient counters stayed at four and consent list stayed empty |
| H-08 HTTP pending-consent reset on 0.8.0 | **FAIL — receipt classification** | Real native prompt subprocess count changed 0 → 1 → 0 around CLI reset; no grant or delivery occurred and counters stayed four, but the terminal receipt incorrectly said `denied`, not `cancelled` |
| HTTP remaining recovery | **NOT RUN** | In-flight cancellation and additional failure cases require separate deliberate observations |

The dedicated service definition was checked for exact executable, root, label,
arguments and private ownership before the restart. Service stop acknowledged
unloading; a read-only status query returned `transport_unavailable`. Service
start and doctor confirmed readiness under a new epoch, the same paired client,
verified application/native-host definitions and one reconnected extension
profile. No vault initialization, key replacement, browser restart or GUI
automation occurred. This is short-outage reconnection evidence, not the full
permission/outage/recovery matrix.

The previously completed HTTP operation returned `not_found` after restart.
It was queried only, never replayed. Recipient counters stayed unchanged until
the separate new operation. Missing status was not treated as proof that an
earlier request had not run.

The pending-reset failure led to a narrow standalone cancellation-receipt fix
and a separate `0.8.1` candidate. See the
[cancellation qualification record](results-cancellation-2026-09-08.md).
The original failure remains recorded; a newer pass must not rewrite its outcome.

The private process recipient exposes only fixed execution markers. It includes
deliberate stdout/stderr credential-echo attempts; the observed CLI output was
closed receipt metadata only. The synthetic enrolled value was not read by the
test operator. The loopback HTTP recipient recorded the approved deliveries
listed above, with nonempty credential headers, and deliberately echoed each
credential in its response body.
The CLI returned only closed receipt metadata. The operator read only the
separate count-only health endpoint, never that echo response. This is local
HTTP acceptance, not public HTTPS/provider or arbitrary-recipient qualification.

Remaining native permission/recovery cases, maximum dialog readability,
browser remembered-consent acceptance, process-grant restart, pending-consent
reset/cancellation, broader host coverage and signed public distribution remain
open. See the [process matrix](process.md),
[extension matrix](extension.md) and [release gates](release.md).
