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

## Committed CI investigation — failure reproduced

[Unsigned run 34184818132, attempt 1](https://github.com/MagicBeansAI/MagicVault/actions/runs/34184818132)
ran source `12432756845cb1e1d65d9da4ccb4f2ea8635efb8` and **FAILED** in
independent installed round 15. Job `101930923350`; macOS `15.7.9` arm64,
image `20260829.0321.1`, Rust `1.92.0`, Node `22.23.2`.

Before that failure, all-target checking, 254 default Rust tests (13 ignored),
87 JavaScript tests, 24 Python tests, architecture review (72 inputs), release
build, the serial 200-launch and async 200-launch/2,000-child-churn probes,
diagnostic unit/classification tests and the explicit release-build refusal
passed. Fourteen complete installed rounds passed. Round 15 passed CLI/native
host cases and the concurrent HTTP case, but failed the process case; later
rounds, performance probes, app-only uninstall and artifact upload did not run.
The run stopped at its first failing lane and was **not retried**. It produced
no downloadable candidate artifact.

Closed observation at the failure:

```text
runtime_entered: true
terminal: RuntimeFailure
dispatch: Dispatched
runtime_error: false
has_exit_code: false
stderr_empty: true
permission_error: false
missing_file_error: false
adapter_returned: true
adapter_state: Uncertain
adapter_error: Unavailable
recipient entered/material-present/completion markers: all absent
```

Static tracing of the locked MagicRun `943ccd2` batch path shows this successful
runtime settlement comes from an exit status without a normal code: on Unix,
signal termination, excluding the separately classified CPU/file-limit signals.
Coordinator/wait/stream errors propagate through the error branch instead; the
result sealer preserves this terminal. This is a **code-based inference**, not
a captured signal number or an identified sender. The observation does not
establish whether the OS, process-group cleanup or another source caused the
termination. Missing entry markers do not prove the executable never started.

The next targeted evidence is the owned child's termination signal and the
before-reap observation/cleanup decision inside MagicRun. Keep that diagnostic
test-only, operation-scoped and value-free. Do not expose raw recipient output,
change cleanup semantics on speculation, automatically replay an uncertain
delivery, or turn the test off to produce a green candidate. No MagicRun change
or runtime fix was made in this investigation.

## Independent installed browser/resource trial

Because the new CI run failed before upload, the browser trial used the latest
available downloaded candidate identified above (`b475f3c`), **not** the failing
revision's newly built binaries. Its release test driver included the new
browser-resource working-tree changes based on `1243275`. Chrome
`152.0.7977.82`; the local host/toolchains and isolation described above apply.

PASS: offline installed CLI/native-host/MCP (13 cases), real native extension
transport with two fresh headless Chrome user-data roots, 20 fills and one
synthetic denial, navigation/block refusal during pending consent, large-profile
discovery with 65 unrelated tabs, independent pause/resume, and 120 seconds of
idle observation. Both profile handles remained connected throughout the final
checks, with no additional host launches or synthetic confirmations in that
window. The application-only recovery/uninstall checks passed; both repacked
tarballs again matched the downloaded originals byte-for-byte.

Native startup observations: `891.82 ms` and `396.77 ms`. Fill observations:
median `30.72 ms`, p95 `35.18 ms`, maximum `37.10 ms`; denial `20.31 ms`.
These include synthetic consent/polling and resource observer perturbation,
not real native-human or provider latency.

| CDP-reported browser process population | Active fill/deny window | Idle window |
| --- | --- | --- |
| Duration / samples | 830 ms / 22 | 120,003 ms / 121 |
| Adjacent-matched CPU seconds | 1.184 | 3.000 |
| Sampled peak resident sum, KiB | 10,232,240 | 10,314,864 |
| Physical footprint sum, first → last KiB | 2,739,155 → 2,876,775 | 2,869,143 → 2,661,042 |
| Sampled peak physical footprint sum, KiB | 2,877,863 | 2,869,143 |
| Process count, peak / last | 89 / 89 | 89 / 83 |
| Missing / regressed counter observations | 0 / 0 | 0 / 0 |
| New / departed process identities | 4 / 2 | 1 / 7 |

These are the **whole sampled Chrome workload**, including 65 unrelated tabs,
not MagicVault-only overhead. Resident sums can count shared pages repeatedly.
CPU excludes unobserved lifetime segments and short-lived processes; process
turnover is visible above. The observer retains a bounded adjacent-sample map,
not a growing process history. Native hosts, MCP, the test broker and standalone
daemon are outside this CDP population. See [method and limits](extension-transport.md#browser-process-resources).
The shared development host was not isolated from other work; a test-driver
rebuild overlapped the end of idle. This is a bounded functionality/resource
observation, not a controlled performance benchmark.

The sampler's PID reuse/missing-counter regression test and the 14 distribution
driver tests passed locally, as did the updated all-target compile check and
architecture gate. A review subsequently hardened counter regression handling
to retain the prior high-water CPU value; its release unit test passed. No
regression occurred in the actual browser trial, so that correction does not
change the recorded numbers. An initial argument-denial test used ambiguous
`--browser-idle-secs -1` syntax; it was corrected to the parser's `=value` form
and rerun. This was a test expectation issue, not a candidate runtime failure.

## Remaining gates

The process signal/source investigation is now narrower but **still open**.
The installed standalone-daemon resource trial, genuine native first-pair denial,
browser restart/long outage, profile/client revocation and interrupted native
upgrade/removal still require an explicitly authorized dedicated installation
and human participation where the OS prompts. The existing `0.8.1` live acceptance
installation was left unchanged. No genuine native gate was replaced with
synthetic consent. Two-minute idle observations do not qualify a long soak.
Signing, registry publication and a public release remain separate gates.
