# Focused process and downloaded-candidate qualification — 2026-09-08

This follow-up builds on MagicVault `42fb42e` and retains MagicRun `25f1c449`.
Changes are confined to test orchestration, the installed-browser CLI test seam,
workflow validation and documentation. No shipped runtime, shared custody,
wire/storage format, Magician or live acceptance installation is changed.

## Independent candidate identity

The newest available successful unsigned distribution was
[run 34187369986](https://github.com/MagicBeansAI/MagicVault/actions/runs/34187369986),
source `01a1cfd97f8d8230c6c7ee710f0970834699dbd2`. Artifact `10041209598` was
downloaded independently. ZIP SHA-256 matched GitHub's recorded digest:
`15c07b224d543d81296268b9a2e9065abda99ef50fa527570c0d4180f2a3a24f`.
Archive entries were inspected before extraction into fresh SSD1 directories.

| Tarball | Independently measured SHA-256 |
| --- | --- |
| `magicvault-local-magicvault-0.8.2.tgz` | `42b2656f10a6be0af7908f55090198d223fa302993dd4441d016b94ca19bb65b` |
| `magicvault-local-magicvault-darwin-arm64-0.8.2.tgz` | `ee8a72d730b1a62232ae6dbc5814bc22a6d99ee3b629a2d6480b11b676de1112` |

Local test-driver source is this follow-up, not the candidate's original source.
The candidate's CLI/MCP/native-host and installed extension inputs remain its
reviewed package bytes. The extension fixture explicitly pregrants loopback only;
synthetic custody/human providers do not qualify genuine keychain or permission UI.
Runtime apps use private internal temporary directories; builds, packages and
fresh browser profiles use SSD1. Personal browsers and the live `0.8.1`
acceptance installation remain outside the test scope.

## Independent installed/browser execution

**PASS** on macOS `26.6` arm64, Rust `1.92.0`, Node `22.19.0`, Chrome
`152.0.7977.82`. All 14 bundle entries matched their recorded lengths and SHA-256
hashes. Both qualifier-packed tarballs matched the downloaded originals
byte-for-byte. This is offline installation, not registry or signer qualification.

- 13 installed CLI/process/HTTP/native-host/MCP default cases passed; three
  separately opt-in performance cases remained ignored in this browser trial.
- Installed CLI → real Chrome CDP: enrollment, configuration, fill and denial
  passed, with synthetic material withheld from client output.
- Installed MCP/native-host → two fresh headless browser profiles: 20 fills,
  denial, navigation/site-block refusal, 65 unrelated-tab discovery and
  independent pause/resume passed. Both profiles remained connected through a
  120-second idle window without additional native-host launches or synthetic
  confirmations in that window.
- npm removal left stable installed paths usable; app-only uninstall preserved
  custody/keychain state and recoverably archived only the disposable app.

Native-host startup: `912.425 ms` and `429.646 ms`. Fill median/p95/maximum:
`24.203/28.319/29.205 ms`; denial `9.347 ms`. These timings include synthetic
consent, polling and observer overhead, not native human interaction.
During idle, 121 samples across 120,002 ms recorded 3.088 matched CPU seconds,
physical footprint sum `2,879,992 → 2,632,530 KiB`, and process population
`89 → 83`; seven new and 13 departed identities, no missing observations or
counter regressions. This is the whole sampled Chrome workload, including the
large-tab fixture, not MagicVault-only overhead or standalone-daemon resources.
It is not a long soak. [Method and limits](extension-transport.md#browser-process-resources).

## Focused process investigation

### Preflight

Local runner preflight passed two fresh trials (four process/HTTP case executions),
three diagnostic units, four real recipient classification cases and the explicit instrumented
release refusal. Both observed recipients exited normally before cleanup and
after reap. This preflight validates direct Cargo-reported driver execution and
its package working directory, not the intermittent failure's cause.

The 18 distribution/orchestration tests, 12 architecture regression tests,
74-input architecture gate and installed-browser CLI selection regression passed.
Failure propagation, no retry, invalid opt-ins and budget exhaustion are covered
by deterministic orchestration tests; no synthetic result replaces actual CI.

### First CI failure retained

[Focused run 34189318693, attempt 1](https://github.com/MagicBeansAI/MagicVault/actions/runs/34189318693)
**FAILED** on source `58592ee5c6579b62e1d8d31265e55b20e181a663`, with the
same locked MagicRun revision. Job `101943932578`; macOS `15.7.9` arm64,
image `20260829.0321.1`, Rust `1.92.0`, Node `22.23.2`.
Uninstrumented clients were built from that revision; the synthetic broker/driver
enabled the two explicit diagnostic cfgs. These are not the downloaded
`01a1cfd` candidate bytes qualified above.

The manual workflow requested at most 200 fresh trials with a ten-minute trial
budget and 25-minute job ceiling. Orchestration tests, the architecture gate,
client build, diagnostic classification and instrumented-release refusal passed
before the loop. Trials 1–41 passed both original process and HTTP companion
cases. **Trial 42 failed the process case; its HTTP companion passed.** The loop
ran for about 33 seconds before stopping. No trial 43, retry, success summary,
post-success uninstall or artifact upload followed. Failure evidence remains on
the original run; only disposable CI installation state was left for runner
teardown. No live app or custody state was involved.

| Observation in the failed process case | Recorded result |
| --- | --- |
| Runtime and launch | Runtime entered; one child spawned |
| First wait, before group cleanup | Matching owned child and `SIGCHLD`; `CLD_KILLED`, `SIGKILL`; one poll, no interruption or error |
| Cleanup | Before-reap cleanup only; no explicit termination cleanup, no group `SIGTERM`; subsequent group `SIGKILL` returned `NoSuchProcess` |
| Final reap | `SIGKILL`, no normal exit status, no wait error |
| Adapter receipt | Dispatched `RuntimeFailure`, `uncertain` / `unavailable`, `may_have_run: true`; no normal exit code, empty stderr |
| Recipient markers | Entry, material-present and completion markers all absent |

The child was already signal-terminated when observed, **before** MagicRun's
recorded group cleanup. That cleanup call therefore did not cause this
occurrence. Static review confirmed the observer runs immediately after
`waitid(WNOWAIT)`, before cleanup and `Child::wait`; timeout/cancellation and
explicit termination take separately observed paths. The executable snapshot
owner is retained through collection; the fixture is a plain `/bin/sh` script,
not a copied platform Mach-O executable. Review did not establish a corrective
runtime change.

The sender or OS termination reason remains **unknown**. The
`spawn_group_owned: None` observation records an unavailable positive group query,
not proof that group setup failed. Missing markers do not prove the child never executed or that no effect
occurred. The matching external symptoms do not prove all earlier intermittent
failures had the same cause. Keep the uncertain receipt and no-replay behavior;
do not disable executable validation, process-group cleanup or OS security to
make this test green.

### Outcome and next evidence

The bounded investigation is complete with a reproduced failure and a narrower
signal/cleanup finding, **not a resolved process reliability gate**. Independent
downloaded-candidate browser/client qualification passed within its synthetic,
single-host scope. The qualification-driver bug was fixed and regression-tested;
no shipped runtime or Magician change was made in this follow-up.

The next useful investigation is an owned-child-only OS termination-reason
observation at the existing pre-reap boundary, with unsupported/denied results
explicitly classified. It needs separate ABI/privacy review and regression
coverage before another bounded run; it is not implemented here. Do not replace
it with broad process/log enumeration or repeat runs without new evidence.
The original [intermittent process finding](results-exact-path-2026-09-08.md)
remains open until a cause is established and addressed. Signing/publication and
genuine native recovery gates remain separate.
