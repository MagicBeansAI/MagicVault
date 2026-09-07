# Native transport and release-candidate qualification — 2026-09-07

**Result:** automated local qualification passed for source candidate `0.6.1` /
extension `0.5.1`. Genuine permission/keychain/service and signed-public-release
gates remain **NOT RUN**. This is conformance evidence, not a security certification
or a claim that an agent can never observe a recipient's credentials afterward.

## Source and environment

- Working-tree changes based on `7eb90ff4819de80799d7b189b65a2b7bafb2e3b7`;
  not an already published revision. The accompanying source, tests and reviewed
  architecture baseline identify the candidate.
- macOS `26.6` (`25G72`), Apple Silicon (`arm64`); Google Chrome `152.0.7977.82`.
- Rust/Cargo `1.92.0`, Node `22.19.0`, npm `10.9.3`; dependency resolution offline.
- CLI/MCP `0.6.1`, extension `0.5.1`; service `0.6.0`, effect `0.4.1`, protocol
  `0.4.0`, core `0.1.3`, primitives `0.1.1` and locked MagicRun `0.1.73` unchanged.
- Cargo.lock SHA-256: `56b77af1d2d349d7410c2d1efa773e604bb4f2cba92bbe9cc1d8379a92ae7c3e`.
- Builds, browser profiles and unsigned npm artifacts on SSD1. Small private
  socket/vault fixtures used OS temp directories; all credentials were synthetic.
  No personal profiles, live vaults, OS-wide host registrations, LaunchAgents or
  keychain entries were changed. No signing, publishing or CI dispatch occurred.

## Executed lanes

| Lane | Result | Scope / limitation |
| --- | --- | --- |
| Full `cargo test --offline --locked --workspace --quiet` | **PASS**, 230 tests; 9 opt-in tests ignored by default | Shared custody/primitive compatibility, standalone APIs/IPC, real child/HTTP/TLS and CLI/MCP/native-host fixtures |
| All extension and script JS tests | **PASS**, 69 tests | Actual worker/options/fill, package assembly/launchers, browser opt-in guard, synthetic Chrome APIs and local fixture contracts |
| `make check` | **PASS** | Workspace/all-target compilation; architecture 0.6.1 matches 64 source inputs; durability guard retains 7 known baseline entries with none added |
| `make test-architecture test-build-paths` | **PASS**, 24 tests | Drift and artifact routing, including new opt-in Cargo lanes |
| `make test-browser-native` | **PASS**, 5 tests, 5.58 s execution | Real headed/headless CDP, controls/partial writes, explicit frame/opaque refusal, stale document and original-tool continuation |
| `make test-cli-native` | **PASS**, 1 test, 3.27 s execution | Actual CLI/daemon/CDP, synthetic allow/deny, canary-free replies/audit |
| `make test-extension-native` | **PASS**, 1 test, 6.34 s execution | Release-build real Chrome/native-host/MCP, two independent user-data roots and synthetic consent |
| `make test-delivery-latency` | **PASS**, 1 test, 8.36 s execution | 20 actual CLI/process and 20 CLI/loopback HTTP operations; output/audit canaries withheld |
| Offline npm `make test-package-browser` | **PASS**, 13 reused client/native-host tests plus 1 actual browser test (5.78 s browser execution) | Installed launchers, native host and extension code; app-only install/activation, npm removal, stable paths and recoverable app uninstall |
| Public demonstration websites | **NOT RUN** in this candidate | Historical evidence exists; no external form or provider login performed |

Eight of the nine opt-in Rust tests were executed separately above. Counts overlap
between normal and packaged lanes; do not add them as unique coverage. The browser
transport fixture uses actual headless Chrome dispatch but **test-only synthetic
human/key providers and a manifest with a single pregranted loopback host**. It
does not qualify Chrome's permission dialog or normal OS-user host installation.

## Functional defect found and corrected

The first real native-transport run established both profile connections but
returned no fill targets despite a valid loopback permission. Chrome exposed
32-character uppercase hexadecimal document IDs; the worker required hyphenated
UUIDs and silently discarded the documents. Earlier mocks supplied only the
hyphenated format and therefore missed this functional failure.

The worker now accepts the observed hex format and retained hyphenated format,
without normalizing either. Exact top/selected-document checks, active lifecycle,
origin policy and `executeScript.documentIds` binding are unchanged. Regression
cases cover uppercase/lowercase hex, separate frame/top tokens, mismatched case,
malformed length/type/characters and trailing newlines. A format match alone is
never authority to fill a different document.

Real-browser assertions additionally verify 20 successful fills and one denial,
reactive input/no submission, navigation or site blocking while synthetic consent
is pending, no write to the replacement/blocked page, blocked target omission,
fresh handles after Pause/Resume, remembered authorization without another prompt,
and survival of the other profile's connection. Receipts/audit contain no canary.

Two early fixture assumptions were corrected as well: production MCP returns a
JSON **text receipt**, not `structured_content`; a site block produces the closed
`denied / permission_denied` outcome, not generic `failed`. Production semantics
were preserved. The corrected source and npm-installed lanes passed.

## Local latency observations

Release builds; 20 sequential successful operations per row, no warm-up discarded.
Median is the mean of the two middle samples; p95 is nearest-rank. Milliseconds
below are rounded to three decimals from the emitted microseconds.

| Path | Min ms | Median ms | p95 ms | Max ms |
| --- | ---: | ---: | ---: | ---: |
| Shipped MCP → native extension → Chrome fill | 21.437 | 25.461 | 27.452 | 33.030 |
| npm-installed MCP/native host/extension → Chrome fill | 18.693 | 20.987 | 24.131 | 24.315 |
| Fresh CLI → daemon → MagicRun child | 173.899 | 195.077 | 296.746 | 584.216 |
| Fresh CLI → daemon → loopback HTTP | 137.493 | 146.352 | 152.724 | 175.745 |

These include receipt/status polling and synthetic consent; CLI rows also include
launching CLI processes. Enrollment, profile setup and browser startup are outside
the samples. Single denial observations were 8.647 ms (source) and 7.211 ms
(packaged). There is no throughput, native-human, remote HTTPS or constant-time
claim; run-to-run differences do not establish a package speedup. No performance
threshold was loosened to obtain a pass. Idle resource use, long soaks, saturation
and peak allocation remain unmeasured.

A final source-lane repeat also passed (5.40 s execution), with median 29.083 ms,
p95 53.590 ms and maximum 56.189 ms. This variability is another reason not to
treat these small samples as a performance guarantee.

## Reproduction

```bash
export CARGO_TARGET_DIR=/Volumes/SSD1/magicvault/builds
export CARGO_BUILD_JOBS=4
export CARGO_NET_OFFLINE=true
export MAGICVAULT_BROWSER_TMPDIR=/Volumes/SSD1/magicvault
export MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
cargo test --offline --locked --workspace --quiet
node --test --test-reporter=spec extension/tests/*.test.cjs scripts/tests/*.test.mjs
make check test-architecture test-build-paths
make test-browser-native test-cli-native
make test-extension-native test-delivery-latency
```

The [transport runbook](extension-transport.md) supplies fresh-directory commands
for the npm lane and explains isolation/cleanup. The tested local tarballs were:

| Unsigned local artifact | SHA-256 |
| --- | --- |
| `magicvault-local-magicvault-0.6.1.tgz` | `5f3f10a9a55d67b64669e8bb4ced99d439816a322671f7d028a7fd26a1e10291` |
| `magicvault-local-magicvault-darwin-arm64-0.6.1.tgz` | `77da939d36ebfa92163192c4e7f572e7aedef5ccabe46330b88b9629b3a4d56c` |

Hashes identify only these local candidates, not publisher authenticity. A future
rebuild or signed candidate must be qualified and recorded separately.

The September 8 tagline/documentation update postdates these tarballs. Their
hashes and install results describe the recorded artifacts, not a newly rebuilt
package containing that updated copy.

## Review boundaries and remaining release gates

Static review covered document identity/discovery/dispatch, consent revalidation,
the trusted native profile handshake, fixture capability isolation, canary-only
receipts, owned-child cleanup, immutable package versions and explicit browser
opt-in. The only production behavior change is the extension document-format fix;
CLI/MCP version bumps allow a new bundle. No service/core/effect runtime, shared
API, key identity, registry/wire schema, MagicRun dependency or Magician source
changed. Shared compatibility tests passed; Magician's own suite was not run.
Final review also corrected the new Makefile lane's handling of build directories
containing spaces: both absolute and relative target paths now retain exact
quoting, covered by the existing fake-Cargo routing test. The corrected routing
suite and actual native-browser lane passed again.

| Gate | Status / next evidence |
| --- | --- |
| Genuine Chrome permission grant/removal and all-HTTPS UX | **NOT RUN**; [extension acceptance matrix](extension.md), with a person |
| Native prompt readability, denial, OS keychain, LaunchAgent and full setup/upgrade recovery | **NOT RUN**; disposable OS account and [native distribution runbook](distribution.md) |
| Browser restart/long outage, real permission revocation, copied-profile and daemon-crash lifecycle under actual Chrome | **NOT RUN** end to end here; retained synthetic lifecycle tests are not a replacement |
| Live Codex/Claude model tool-selection and account-specific setup | **NOT RUN**; official SDK stdio and installed MCP transport passed, no live agent configuration/account changed |
| Developer ID, notarization, quarantined download, npm provenance/publication, Store identity | **NOT RUN**; separate release authority and exact artifact verification required |
| Wider browsers/platforms, resource/soak benchmarks and consumer-owned runtime suites | **NOT RUN**; no extrapolation from this host |
| Running-process/stateful-service refresh and browser-output mediation | **NOT IMPLEMENTED**, unchanged product boundary |

Remaining gates stay visible in public docs. Passing local qualification does not
authorize publication or justify “nobody can ever read your credentials.” The
supported promise remains reference-only agent inputs and value-free MagicVault
receipts, with plaintext delivered only through the approved recipient boundary.
