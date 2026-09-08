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

The bounded CI run is pending at this initial record. Local runner preflight
passed two fresh trials (four process/HTTP case executions), three diagnostic
units, four real recipient classification cases and the explicit instrumented
release refusal. Both observed recipients exited normally before cleanup and
after reap. This preflight validates direct Cargo-reported driver execution and
its package working directory, not the intermittent failure's cause.

The 18 distribution/orchestration tests, 12 architecture regression tests,
74-input architecture gate and installed-browser CLI selection regression passed.
Failure propagation, no retry, invalid opt-ins and budget exhaustion are covered
by deterministic orchestration tests; no synthetic result replaces actual CI.
The original [intermittent process finding](results-exact-path-2026-09-08.md)
remains open until a cause is established and addressed; an inconclusive bounded
run does not close it. Signing/publication and genuine native recovery gates are
separate.
