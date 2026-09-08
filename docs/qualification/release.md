# Public release acceptance

Scope: the first prebuilt **macOS Apple Silicon** CLI/MCP/native-host bundle and
its unpacked Chromium extension. Passing library tests on Linux does not qualify
a Linux desktop product. This is an acceptance checklist, not a promise that all
gates have passed or a substitute for the [security boundary](../../SECURITY.md).

## Gates and evidence

Latest unsigned candidate: [signal-observer CI on `01a1cfd`](results-process-signal-2026-09-08.md)
passed normal checks/builds, 20 installed rounds and bounded performance/shutdown
tests. It did not reproduce or fix the earlier process failure, execute genuine
native acceptance, or produce a signed/public release.

| Gate | Current evidence | Required before claiming completion |
| --- | --- | --- |
| Committed source, architecture and automated conformance | [Standalone `0.8.2` correction, full local tests and first-attempt macOS/Ubuntu/distribution CI on `23e648c`](results-completion-order-2026-09-08.md); prior failures remain separately identified | Requalify changed inputs and the final release revision; do not erase intermittent failures or attribute older CI to new binaries |
| Offline prebuilt installation | [Unsigned `0.8.2` candidate](results-completion-order-2026-09-08.md) passed 20 CI installed rounds; downloaded artifact hash and an independent installed browser/client trial verified | Qualify the actual final signed packages and quarantined download; unsigned artifact success is not publisher/Gatekeeper acceptance |
| Real browser dispatch and profile independence | Headed/headless CDP, shipped CLI, two-profile native extension/MCP, repeated fresh starts; [external-volume OS startup limitation](results-startup-policy-2026-09-08.md) | Qualify the internal runtime installation; external native-host startup is not qualified for unattended use. Keep unsampled failures visible and complete remaining [native extension cases](extension.md) |
| Genuine installed keychain and browser consent | [One-host basic acceptance](results-discovery-2026-09-08.md); [remembered-use permission removal/restoration and pause/resume](results-permission-recovery-2026-09-08.md) | First-pairing denial, full-browser restart/long outage, native profile/client revocation, wider permission combinations and remaining recovery cases |
| New-process and HTTP delivery | Automated conformance plus [genuine modes and revocation](results-consent-2026-09-08.md), [pending cancellation and process-grant restart](results-cancellation-2026-09-08.md), and [actual in-flight cancellation](results-permission-recovery-2026-09-08.md) | Resolve [reproduced exact-path process failure in CI round 15](results-exact-path-2026-09-08.md): dispatched `RuntimeFailure`, no exit code, empty stderr, no entry marker; termination signal/source still unknown. Remaining native cases are in the [runbook](process.md) |
| Installation recovery and removal | Synthetic ownership, corruption, activation, drain and recoverable-retirement tests; packaged app-only lifecycle | [Native lifecycle](distribution.md), interrupted upgrade/drain and managed removal in a disposable account or explicitly authorized dedicated installation |
| Performance and reliability | [Corrected `0.8.2` installed load/capacity/shutdown observations](results-completion-order-2026-09-08.md) and [two-profile real-browser active/120-second idle process counters](results-exact-path-2026-09-08.md); original failures retained | Resolve the process failure; installed standalone-daemon/native-host resources and a long soak remain unqualified. Browser counters exclude those processes, include the large-tab fixture and can miss transient peaks; no production guarantee |
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
