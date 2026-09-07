# Large-profile discovery qualification — 2026-09-08

**Result:** source and packaged qualification passed for CLI/MCP/service
`0.7.0`, protocol/effect `0.5.0`, extension `0.6.0`. The dedicated installed
acceptance daemon is ready after human keychain approval, with the existing
pairing and native-host registration intact. The human loaded extension 0.6.0;
MCP narrowed discovery now completes without `capacity`. After human fixture-site
setup/permission it returns exactly one matching main-frame target. Genuine
native denial returned `denied / not_filled`; a separate human-approved operation
returned `filled` with no error. Post-fill diagnostics confirm the daemon and
extension remain healthy. This is an
unsigned local candidate, not a published release or a security certification.

## Source and environment

- Uncommitted working tree based on
  `7eb90ff4819de80799d7b189b65a2b7bafb2e3b7`.
- macOS `26.6` (`25G72`), Apple Silicon; Chrome `152.0.7977.82`;
  Rust/Cargo `1.92.0`, Node `22.19.0`, npm `10.9.3`.
- Builds, disposable Chrome profiles and packages on SSD1. Private IPC fixtures
  use OS temporary directories. All acceptance credentials are synthetic.
- Cargo.lock SHA-256:
  `f7223a7facee9381a8ab8453f993ace084c42cb7c98a0884921a64fa31987088`.
- Shared custody core/primitives, vault/key identities, MagicRun and Magician are
  unchanged. Standalone protocol, service, adapters, CLI/MCP and native host
  change to carry and validate discovery narrowing. Existing unfiltered JSON,
  fill consent and credential-delivery semantics remain unchanged.

## Defect, first candidate and course correction

The former worker rejected discovery above 64 **total** tabs, including tabs
without usable host permission. Candidate `0.6.2` / extension `0.5.2` removed
that total-profile gate: query granted URL patterns and loaded tabs, filter
blocked/unsupported/inaccessible candidates, then apply the inspection budget.
It passed 80 JavaScript tests and source/packaged Chrome tests with 65 unrelated
tabs. The installed app upgrade also passed.

Chrome Reload nevertheless retained the old resolved bundle directory and
extension `0.5.1`. Loading the updated fixed-ID assets changed the displayed
version to `0.5.2`, preserving profile identity and existing grants. Discovery
still returned `capacity`: filtering unrelated tabs alone did not solve an
all-HTTPS profile exceeding the remaining eligible-tab/target limits.

Candidate `0.7.0` therefore adds optional exact `top_origin` and backend
`tab_id` narrowing, wired through MCP/CLI, broker, native host/extension and CDP.
Both filters must match. Apply them before inspection and recheck actual
document metadata afterward; the bridge and broker reject out-of-filter replies.
A caller's filter never grants browser access or credential-use authority.
Chrome patterns ignore ports, so explicit origin comparisons preserve exact ports.

Bounds remain explicit: 128 eligible tabs, 128 returned targets, and the
existing per-tab frame budget. A too-large matching set returns `capacity`,
not a truncated list. There is no pagination or unlimited-profile claim.
Discarded tabs are not awakened. CDP retains its connection after a clean local
discovery overflow so a narrower query can follow; transport/frame failures
still invalidate it. This exception never retries a fill.

Tests cover 500 unrelated/all-granted tabs, 65/128/129 boundaries, exact ports,
conjunctive filters, malformed input, grants/blocks, navigation, permission races,
cache lifetime, stale channels, out-of-filter bridge replies, CDP overflow
recovery, broker handle invalidation and backward-compatible unfiltered JSON.
Real Chrome retains 65 unrelated tabs and alternates filtered/unfiltered MCP
discovery. The real CLI/CDP lane supplies both filters before delivery.

## Executed 0.7.0 lanes

| Lane | Result | Scope / limitation |
| --- | --- | --- |
| Full `cargo test --offline --workspace --quiet` | **PASS**, 235 tests; 9 opt-in tests ignored | Shared compatibility, standalone APIs, real child/HTTP/TLS and IPC fixtures; lockfile changes only local component versions |
| Discovery/worker JavaScript | **PASS**, 35 tests | 15 discovery cases plus retained worker tests |
| All extension/script JavaScript | **PASS**, 84 tests | Actual worker/options/fill and packaging fixtures; local loopback access permitted |
| `make build-standalone` | **PASS**, 21.75 s | Matching release CLI, MCP and native host |
| `make check test-architecture test-build-paths` | **PASS**, 24 guard tests | All-target compilation, reviewed 64-input baseline, 7 known durability baseline entries with none added |
| `make test-browser-native` | **PASS**, 5 tests, 2.83 s execution | Actual headed/headless CDP and document/control regressions |
| `make test-cli-native` | **PASS**, 1 test, 1.82 s execution | CLI narrowing through daemon/CDP; synthetic allow/deny and value-free output |
| `make test-extension-native` | **PASS** on repeat, 1 test, 4.90 s execution | Two disposable Chrome roots, 65 unrelated tabs, 20 fills, denial and navigation/block/reconnect checks |
| `make package-npm test-package-browser` | **PASS**, 13 reused client/host tests plus 1 real browser test (13.25 s browser execution) | Offline install, matching installed assets, lifecycle and npm-removal checks |

The first 0.7.0 extension run timed out at initial connection setup after 15.71 s,
before discovery. Its cause was not established. Value-free connection-state
diagnostics were added without extending the deadline; the source repeat and
packaged lane passed. Preserve this observation as a startup-reliability risk,
not a diagnosed/fixed failure.

Counts overlap; do not sum them as unique coverage. Automated native lanes use
synthetic human/key providers and a pregranted loopback host in disposable
profiles. They do not qualify genuine permission/keychain UI.

For 20 sequential successful operations, source/native median was 26.226 ms,
p95 32.379 ms, maximum 50.381 ms. Packaged median was 25.860 ms, p95 32.468 ms,
maximum 33.082 ms. These include receipt polling and synthetic consent, not human
reaction time or browser startup. Small samples establish no throughput,
memory/soak, native-UI or production performance guarantee.

## Installed native acceptance

The user explicitly authorized a dedicated acceptance app/vault in the current
OS account because another logged-in desktop was unavailable. Synthetic
credential enrollment used a hidden native prompt; the agent did not receive
its value. Only that app/vault/profile and its matching native-host definitions
were in scope. No unrelated tabs, grants or security settings were changed to
make discovery pass. This is an explicitly authorized exception, not the default
disposable-account runbook.

| Case | Observed result |
| --- | --- |
| Native keychain initialization, pairing, hidden synthetic enrollment | **PASS** in the preceding 0.6.1 session; values not inspected |
| Managed upgrade 0.6.1 → 0.6.2 | **PASS**; service restarted, old bundle retained, vault unchanged |
| Fixed-ID reload from new directory | **PASS** for extension 0.5.2; profile identity and existing grants retained |
| Unfiltered existing-profile discovery at 0.6.2 | **FAIL**, still `capacity`; motivated explicit narrowing |
| Managed upgrade 0.6.2 → 0.7.0 | Bundle activated and service launched, but command returned `transport_uncertain` during the 15-second readiness wait |
| Native keychain prompt after upgrade | Human completed it; a subsequent read-only `doctor` confirmed **ready**, version/integrity verified, same client pairing and matching host registration |
| Matching extension 0.6.0 / narrowed MCP dispatch | **PASS** for request handling; human confirmed loaded version, profile reconnected, exact local-origin query completed without `capacity` and returned zero targets. This is not yet positive target/fill acceptance |
| Local fixture tab/site grant, then positive target discovery | **PASS**; human opened the loopback fixture and granted its site, then shipped MCP returned exactly one matching main-frame target with the exact top/frame origin and no `capacity` error |
| Native credential destination rule | **PASS**; human approved only the synthetic credential's password field and exact loopback origin |
| Native per-use denial through installed MCP/extension | **PASS**; first operation settled as `denied`, field `not_filled`, closed error `denied` |
| Native per-use allow through installed MCP/extension | **PASS**; a separate fresh operation, target and native approval settled as `filled`, field `filled`, error `null` |
| Post-fill `doctor` | **PASS**; daemon ready, same epoch/client, app integrity verified, one connected profile and matching host definitions; no next steps |
| First-pairing denial, permission removal and full recovery matrix | **NOT RUN** |

Both effect requests contained only the stored credential reference, field name,
strict `#password` selector and daemon-issued handles. Neither MCP reply included
the credential; captured MCP stderr was empty. The fixed browser function checks
the assigned value against the supplied value inside the recipient boundary
before reporting `filled`; the agent did not read the password or inspect the
page's password value. No form submission was requested. Independent visual
confirmation of the fixture event/submission counters was not collected; it is
not implied by the receipt. A user-enrolled synthetic password need not equal
the fixture's public canary, so its canary-match indicator may correctly be false.

The denied operation was not replayed. A new operation was used only for the
explicitly separate allow case after denial had settled. No missing, pending or
uncertain effect was treated as safe to retry. This qualifies the basic installed
keychain/permission/consent/delivery path on one host, not the complete acceptance
matrix, live agent-model behavior, arbitrary websites or a signed release.

The post-upgrade daemon initially waited inside a keychain call before creating
its sockets. The human reported difficulty keeping focus on the password prompt;
the GUI automation helper was stopped, the human completed the prompt, and the
daemon became ready without another upgrade or reinitialization. The exact focus
interference was not isolated. Keep automation off during native password entry.

The upgrade guide now documents readiness timeout reconciliation with
`doctor`, and Chrome retaining an old resolved path across a `current` symlink
switch. Verify Chrome's extension version and use **Load unpacked** for the new
directory when necessary. Do not remove the existing fixed-ID extension first:
that resets local pairing, grants and blocks. Native prompts remain human-only.

## Artifacts and reproduction

| Unsigned local artifact | SHA-256 |
| --- | --- |
| `magicvault-local-magicvault-0.7.0.tgz` | `68918b9b046526784634c7d31918bab99f0f7c8f497f60d34f964674fac7bca1` |
| `magicvault-local-magicvault-darwin-arm64-0.7.0.tgz` | `84790a6bb8a2cbf402189d32f69af8c68cd4178d739b977d59dc76ef44d70a58` |

Hashes identify tested local artifacts, not publisher authenticity. This
qualification/recovery-documentation update postdates those artifacts; runtime
and extension sources are the tested versions. A rebuild/signing step requires
its own artifact identification and qualification.

```bash
export CARGO_TARGET_DIR=/Volumes/SSD1/magicvault/builds
export CARGO_BUILD_JOBS=4
export CARGO_NET_OFFLINE=true
export MAGICVAULT_BROWSER_TMPDIR=/Volumes/SSD1/magicvault
export MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
cargo test --offline --locked --workspace --quiet
node --test --test-reporter=dot extension/tests/*.test.cjs scripts/tests/*.test.mjs
make check test-architecture test-build-paths
make build-standalone test-browser-native test-cli-native test-extension-native
```

Follow the [transport runbook](extension-transport.md) for package assembly and
`make test-package-browser`. No signing, notarization, publication, Git push,
live agent-model configuration or Magician test run occurred.
Remaining [native/release gates](README.md) are not waived by these results.
