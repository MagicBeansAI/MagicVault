# Public release acceptance

Scope: the first prebuilt **macOS Apple Silicon** CLI/MCP/native-host bundle and
its unpacked Chromium extension. Passing library tests on Linux does not qualify
a Linux desktop product. This is an acceptance checklist, not a promise that all
gates have passed or a substitute for the [security boundary](../../SECURITY.md).

## Gates and evidence

Latest independently downloaded candidate: [0.8.3 on `81fe8c6`](results-broad-candidate-2026-09-08.md).
Full macOS/Linux conformance and unsigned distribution CI passed on their first
attempts, including 20 installed rounds and bounded performance/shutdown tests.
The downloaded ZIP, both tarballs and all 14 bundle entries were verified.
Independent installed CLI/MCP and real-CDP cases passed, but the first native
extension trial **failed at a browser-command timeout during setup**. That run
did not reach idle/resource observations. One fresh test-diagnostic trial passed
two-profile extension, 120-second idle and local delivery/shutdown checks. The
retained failure and follow-up are in the new record; broader browser reliability is not
closed by passing CI or a subsequent individual trial.

The [browser-command follow-up](results-browser-command-2026-09-08.md) reproduced
later evaluation, discovery and fill-settlement timeouts after both profiles
connected. A ten-trial Chrome-only control and final ten-round installed-candidate
run passed. The browser gate remains open: no cause or runtime fix was established,
and the Chrome-only control exercises neither extension APIs nor native transport.

The [0.8.3 native-spawn correction on `1baf57b`](results-native-spawn-2026-09-08.md)
passed all 200 original concurrent process/HTTP trials on its first CI
attempt, explicitly confirming the native backend. This qualifies the scoped
launch correction; the historical exception remains unattributed and earlier
failures remain in their original records. Neither workflow qualifies genuine
native acceptance or produces a signed/public release.

| Gate | Current evidence | Required before claiming completion |
| --- | --- | --- |
| Committed source, architecture and automated conformance | [Full 0.8.3 macOS 26/Linux conformance and macOS 15 distribution CI on `81fe8c6`](results-broad-candidate-2026-09-08.md); [200-trial focused macOS CI](results-native-spawn-2026-09-08.md) | Re-run applicable gates when product/package bytes change; test-only follow-up evidence is explicitly distinguished from the original committed CI |
| Offline prebuilt installation | [Unsigned 0.8.3 candidate](results-broad-candidate-2026-09-08.md): downloaded ZIP/tarball hashes and all 14 bundle entries verified; installed CLI/MCP and real-CDP trial passed | Qualify the actual final signed packages and quarantined download; unsigned artifact success is not publisher/Gatekeeper acceptance |
| Real browser dispatch and profile independence | [0.8.3 CDP and two-profile/idle passes with initial failure retained](results-broad-candidate-2026-09-08.md); [post-connection failures, Chrome control and final ten-round pass](results-browser-command-2026-09-08.md); [external-volume limitation](results-startup-policy-2026-09-08.md) | Resolve the intermittent browser/extension failures without conflating distinct boundaries or historical native-host startup. External native-host startup is not qualified for unattended use. Complete remaining [native extension cases](extension.md) |
| Genuine installed keychain and browser consent | [One-host basic acceptance](results-discovery-2026-09-08.md); [remembered-use permission removal/restoration and pause/resume](results-permission-recovery-2026-09-08.md) | First-pairing denial, full-browser restart/long outage, native profile/client revocation, wider permission combinations and remaining recovery cases |
| New-process and HTTP delivery | [0.8.3 fresh distribution, installed delivery and bounded shutdown passes](results-broad-candidate-2026-09-08.md); [scoped native-spawn correction and 200 concurrent CI trials](results-native-spawn-2026-09-08.md); historical [genuine consent](results-consent-2026-09-08.md) and [in-flight cancellation](results-permission-recovery-2026-09-08.md) remain version-specific | Qualify final-artifact native lifecycle and wider hosts; the exact historical exception is unattributed, not grounds to replay uncertain work. Native cases are in the [runbook](process.md) |
| Installation recovery and removal | Synthetic ownership, corruption, activation, drain and recoverable-retirement tests; packaged app-only lifecycle | [Native lifecycle](distribution.md), interrupted upgrade/drain and managed removal in a disposable account or explicitly authorized dedicated installation |
| Performance and reliability | [0.8.3 CI installed latency, load/capacity/shutdown observations](results-broad-candidate-2026-09-08.md); [retained browser failures](results-browser-command-2026-09-08.md) | Resolve intermittent browser/extension deadlines; installed standalone-daemon/native-host resources and a long soak remain unqualified. Browser counters exclude those processes and can miss transient peaks; no production guarantee |
| Publisher identity and Gatekeeper | Signing procedure exists; current candidates are unsigned/ad-hoc | Explicitly authorized Developer ID identity and notary profile; notarize and qualify a fresh quarantined download of the exact final bytes |
| Public npm installation and provenance | Local-only `@magicvault-local` candidate scope | Maintainer-selected and verified owned scope, protected trusted publishing, native package then matching launcher, registry/provenance/install verification |
| Public GitHub release and onboarding | Concise MCP-first README and technical guides | Reviewed release notes/checksums, a deliberately authorized release/tag, usable download/install commands and matching repository description |
| Real agent-client acceptance | Official-SDK stdio/MCP conformance | A deliberately configured disposable client session; tool selection and model behavior must not be inferred from transport tests |

No signing, notarization, registry publication or public GitHub release is implied
by pushing a commit or dispatching the unsigned candidate workflow. An Actions
artifact is a temporary qualification download, not a published release.

## Run the non-interactive gates

Use a trusted Chrome executable and disposable test roots. Keep the installed
acceptance app separate; none of these commands should target a personal vault.

```bash
export CARGO_TARGET_DIR="$(make -s print-target-dir)"
export CARGO_BUILD_JOBS=4
export MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
# Optional existing parent for fresh disposable browser directories:
export MAGICVAULT_BROWSER_TMPDIR=/Volumes/SSD1/magicvault
make check test test-extension test-build-paths test-architecture
make test-delivery test-delivery-latency test-qualification-fixtures
make build-standalone test-browser-native test-cli-native test-extension-native
```

Dispatch **Manual qualification** and **Unsigned distribution candidate** on the
reviewed revision. Record each run's actual `head_sha` and terminal conclusion;
an older green run does not qualify a new commit. Both workflows are explicit
opt-ins and do not sign or publish. For local package reproduction, use the
[packaged-browser commands](extension-transport.md#qualify-the-actual-npm-installed-assets).

## Human and publisher gates

Continue the remaining native permission/recovery, recipient and distribution
lifecycle cases; the linked passes above cover only their recorded scopes. Use
only synthetic credentials and inspect recipient-side booleans/markers, never the value. Do not
replay a denied, missing or uncertain effect as part of reconciliation: a separate
allow case uses a fresh operation after the earlier case has settled.

Do not remove a working fixed-ID extension just to reload updated assets. Preserve
its pairing, permissions and blocklist; verify its displayed version and use
**Load unpacked** for the updated directory when needed. Keep GUI automation off
during native password entry.

Selecting a public npm namespace, using a signing identity, submitting to Apple,
publishing packages and creating a public release each require explicit maintainer
authority. Never use test success as that authority. Do not weaken Gatekeeper,
consent, site permissions or destination checks to make a release gate pass.

For final evidence, name the source revision, toolchains, platform, exact artifact
hashes, executed cases, genuine versus synthetic consent, failures and cleanup.
Keep capabilities, signing credentials/logs, browser profiles and raw traces out
of public records. The [result template](result-template.md) is intentionally
value-free.
