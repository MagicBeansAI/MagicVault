# Exact-path process diagnostics — 2026-09-08

## Scope and artifact

The original [intermittent dispatched process uncertainty](results-distribution-ci-2026-09-08.md)
remains **unresolved**. Prior serial/async stress and passing installed-client
trials did not reproduce its terminal category. This investigation instruments
the exact failing integration fixture, not the public CLI's output contract.

The local trial used the downloaded unsigned `0.8.2` candidate from
[run 34182556083](https://github.com/MagicBeansAI/MagicVault/actions/runs/34182556083),
source `b475f3c32090b018d7f0378f1c5aa4d13d5dfeec`, artifact `10039541723`.
Downloaded ZIP SHA-256:
`d51c47a7d72734ae256419b791241e5544470d8bd57c27a74b1ff081e53fe750`.
The two reviewed tarballs were unpacked into fresh package inputs and installed
offline. Byte-for-byte comparison confirmed both qualifier-packed tarballs match
the downloaded originals. The diagnostic driver came from this record's working-tree changes
based on `d252a44`; it is not represented as the CI artifact's original source.

Host: macOS `26.6`, Apple Silicon, Rust `1.92.0`, Node `22.19.0`.
Builds and package work used SSD1; the fresh disposable runtime app used the
internal temporary volume. No personal vault, native keychain, installed daemon,
browser permissions or existing acceptance installation was changed.

## Executed locally

| Case | Result |
| --- | --- |
| Observer registration, capacity release, thread/operation isolation | PASS — two unit tests |
| Real synthetic recipients: nonzero exit, signal termination, non-executable file, missing file | PASS — four cases in one integration test; no canary in the formatted snapshot |
| Diagnostic driver build | PASS |
| Standard release compilation with diagnostic cfg | Correctly REFUSED by the explicit guard; other compiler failures cannot satisfy this check |
| Offline installed CLI/native-host/MCP, 20 independent fail-fast rounds | PASS — 13 default test executions per round, 260 total; three explicit performance tests per round remain ignored here |
| Exact process-path capture | Observed successful runtime settlement and adapter completion; original intermittent failure not reproduced |
| Recoverable app-only uninstall and npm removal | PASS; no live custody or services touched |

The debug-only observer registers at most 16 explicit synthetic operation IDs.
It retains closed enums/booleans, not stream bytes, raw exit codes, paths or
credentials. Known-error flags are string-pattern classifications, not proof of
an operating-system cause. Normal builds omit the hooks and observer; the
qualifier uses a separate diagnostic target and verifies the standard release
guard. No shared core, primitives, MagicRun or Magician change is required.

## Remaining evidence

The reviewed diagnostic revision still needs its own bounded CI run. A passing
run must not be described as fixing the original process uncertainty. The
downloaded candidate also needs the separate real-browser/resource trial; this
record does not qualify genuine native consent, installed standalone-daemon
resources, a long soak, signing, registry publication or a public release.
