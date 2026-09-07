# Broader release qualification — 2026-09-08

Source revision: `f7a71cb2eeabc87c64f769931f68636c7dfc8b67`, pushed to `main`.
Subsequent working-tree changes are qualification documentation only. Component
versions remain CLI/MCP/service `0.7.0`, protocol/effect `0.5.0`, extension `0.6.0`,
core `0.1.3`, primitives `0.1.1` and the locked MagicRun dependency `0.1.73`.
No shared-core, MagicRun or Magician source was changed in this qualification.

Local environment: macOS `26.6` (`25G72`), Apple Silicon, Chrome `152.0.7977.82`,
Rust/Cargo `1.92.0`, Node `22.19.0`, npm `10.9.3`. Builds and disposable browser
roots use SSD1; private IPC fixtures use OS temporary directories. Cargo used the
existing lockfile offline. Its SHA-256 remains
`f7223a7facee9381a8ab8453f993ace084c42cb7c98a0884921a64fa31987088`.

## CI on the committed revision

| Run | Result | Boundary |
| --- | --- | --- |
| [Manual qualification](https://github.com/MagicBeansAI/MagicVault/actions/runs/34161885278) | **PASS**, macOS and Ubuntu jobs | Build-path/architecture guards, all-target compilation, full workspace tests, extension JavaScript and local fixtures; no native desktop UI qualification |
| [Unsigned distribution candidate](https://github.com/MagicBeansAI/MagicVault/actions/runs/34161886971) | **PASS** | Matching prebuilt packages, offline installed executable/IPC qualification and artifact upload; no signing, notarization or publication |

Both runs were explicitly dispatched and report the source SHA above. Previous
green runs on `7eb90ff` were not reused as evidence for this revision. Library
success on Ubuntu does not imply a Linux native-keychain/desktop release.

The distribution run executed the full 235-test Rust suite (9 explicit opt-in
cases ignored), then reused 13 client/host/IPC tests against installed binaries.
Its final package report confirmed `passed: true`, `version: 0.7.0`,
`live_custody_or_services_touched: false`, and `isolated_browser_tests: false`.
Real browser results below come from the separate local lane, not this workflow.

The [unsigned workflow artifact](https://github.com/MagicBeansAI/MagicVault/actions/runs/34161886971/artifacts/10032966716)
contains the two local-scope npm tarballs. GitHub reported artifact ID
`10032966716`, 6,411,366 bytes and ZIP SHA-256
`afbfb4948a682bc822ee52fe296978e8eea796bd5a0cf4560c8b7499b4fc92d3`.
These are workflow metadata and upload-log observations, not an independent
download verification or publisher signature. The artifact expires on
2026-09-21 (UTC); no GitHub release was present when checked.

## Additional local execution

| Command / case | Result | Scope |
| --- | --- | --- |
| `make test-delivery` | **PASS**, 32 Rust tests; one opt-in latency case initially ignored | Real HTTP/TLS and children, closed profile authority, output withholding, cancellation, lost replies, executable changes, broker persistence and shipped CLI/MCP |
| `make test-delivery-latency` | **PASS**, the explicit ignored case executed | 20 fresh process and 20 fresh loopback HTTP operations; synthetic consent |
| All extension/script JavaScript | **PASS**, 84 tests | Worker, discovery, options, fill, packaging and local fixtures |
| `make test-qualification-fixtures` | **PASS**, 5 tests | Synthetic recipient/server behavior; included in the JavaScript count, not a running-service product claim |
| `make build-standalone` | **PASS** | Matching release CLI/MCP/native host |
| `make test-browser-native` | **PASS**, 5 tests, 3.81 s | Headed/headless Chrome, controls, navigation and frames |
| `make test-cli-native` | **PASS**, 1 test, 2.48 s | Shipped CLI/daemon/CDP and synthetic custody/UI |
| Three independent native-extension/MCP trials | **PASS**, 3/3; 5.51 s, 4.82 s, 3.78 s | Each trial uses two fresh browser roots, 20 successful fills, denial, navigation/block/pause/reconnect and 65 unrelated tabs |
| Installer library tests | **PASS**, 10 tests, 0.89 s | Private copying, ownership/overlap, corruption, staged revalidation, retained versions and recoverable retirement in synthetic directories |
| Setup/doctor tests | **PASS**, 6 tests, 2.01 s | Synthetic writer-drain and diagnostic cases; no OS service/keychain changes |
| Build-path and architecture tests | **PASS**, 24 tests | Reviewed architecture `0.7.0`, 64 source inputs; whitespace check also passed |
| Post-qualification installed `doctor` | **PASS** | Dedicated acceptance daemon ready, same epoch/client, app integrity verified, one connected extension profile, matching native-host definitions and no next steps; read-only |

Counts overlap and must not be added as unique coverage. CI full-suite results
and these focused rechecks are separate executions of shared tests.

### Small-sample timing, not a performance guarantee

| Surface / trial | Samples | Median | p95 | Maximum |
| --- | --- | --- | --- | --- |
| CLI process delivery | 20 | 184.786 ms | 233.241 ms | 437.542 ms |
| CLI loopback HTTP delivery | 20 | 151.727 ms | 161.747 ms | 188.581 ms |
| Extension/MCP cold-start trial 1, successful fills | 20 | 53.217 ms | 70.300 ms | 222.505 ms |
| Extension/MCP cold-start trial 2, successful fills | 20 | 53.983 ms | 68.751 ms | 73.939 ms |
| Extension/MCP cold-start trial 3, successful fills | 20 | 27.227 ms | 44.874 ms | 45.483 ms |

Each trial includes fresh browser startup, but its **fill** samples exclude
startup and human reaction time. CLI samples include process launch and receipt
polling. Consent/key providers are synthetic. The host was not controlled for
other load; these are observations, not a regression threshold, Internet latency,
soak, throughput or CPU/memory qualification.

The earlier one-off extension startup timeout remains recorded in the
[discovery qualification](results-discovery-2026-09-08.md). Three successful
independent trials did not reproduce it; they do not establish its cause or fix it.

## Native and public-release limits

The earlier [installed one-host acceptance](results-discovery-2026-09-08.md)
qualifies basic keychain setup/enrollment, upgrade, site permission and native
browser deny/allow. It was not repeated or silently broadened here. No native
process/HTTP prompt, first-pairing denial, permission removal, long outage,
interrupted native upgrade or uninstall was performed in this pass.

The distribution runbook's stale remove/reload instruction was corrected:
load updated fixed-ID assets without first removing the extension, and verify
the version, connection, grants and blocks. Removal resets local settings.

Signing/notarization, quarantined-download Gatekeeper acceptance, npm scope and
trusted-publisher setup, registry publication, public release creation and live
agent-client acceptance remain open. See the [release gate matrix](release.md).
Temporary workflow artifacts are not public releases or publisher verification.

All local browser/server/child processes owned by the completed tests were cleaned
up by their fixtures. The existing human acceptance daemon, extension, pairing,
site grants and vault were left intact. No live credential, personal browser
profile, signing identity or public registry was used by these tests.
All 36 relative file-link targets in the new/updated qualification documents
were checked for existence; this was not an external-link or anchor validation.
