# Browser-command timeout investigation — 2026-09-08

This follows the retained [fresh 0.8.3 candidate failure](results-broad-candidate-2026-09-08.md).
It uses the same independently verified artifact `10045954485`, source
`81fe8c61246da33597aedb4c29535080709af3be`, and its exact locked MagicRun
`af348ab566cbf495f59d155a328bf2cac6afa09d`. Downloaded package hashes remain
identical to the preceding record; all 14 bundle entries were checked again.
Local test drivers include the preceding uncommitted, value-free command labels
and startup checkpoints. Product code, package bytes, shared custody and
Magician are unchanged.

Environment: macOS `26.6` arm64, Rust `1.92.0`, Node `22.19.0`, Chrome
`152.0.7977.82`. Builds, logs and newly created browser profiles use SSD1;
the disposable installed app uses the internal temporary volume. All custody
and consent are synthetic. No personal browser, keychain, LaunchAgent, native
permission change, stack sampling or general process/OS-log discovery is used.

## First bounded investigation — failure retained

The existing package qualifier requested at most 20 fresh rounds with
`--with-rust-tests --with-browser-tests --reliability-rounds 20 --app-parent /private/tmp`.
All original command/connection deadlines and fixture operations were retained;
idle/performance lanes were not selected for this startup investigation.

**FAIL in round 1; no round 2.** All 13 installed CLI/process/HTTP/native-host/MCP
cases passed, followed by installed CLI → real Chrome (1.93 seconds). Both
independent extension profiles passed every setup checkpoint and connected in
3.684 seconds and 2.350 seconds. A subsequent command then failed with
`test browser command timed out (Runtime.evaluate)`. The extension case ended
after 16.80 seconds. There was no success summary, npm removal or app-only
uninstall; the failed trial's isolated application and local log remain retained.

This occurrence is **after both native connections**, unlike the original
11.79-second failure, which had no completed startup summary. The command label
does not distinguish status-message evaluation from fixture-page evaluation.
It does not establish a common cause, a native-host startup defect, a denied OS
permission, a process-delivery regression or safe replay of any uncertain work.

## Failure-only phase observation

Added failure-only fixture phase and bounded iteration labels, without collecting
expressions, page contents, native frames, capabilities or response payloads.
The observer stores only the latest closed enum and optional index (0–20),
prints on unwind only, and changes no control flow or deadline. A fresh bounded
run can then identify the actual timed-out operation. Its bounded-state
regression passed.

The phase-instrumented run passed rounds 1–3, then **failed in round 4**, with
`Runtime.evaluate` and phase `BlockRace`; no round 5 ran. All four rounds passed
the 13 installed-client cases and installed real-CDP case. The failed extension
case had connected both profiles (814.509 ms / 381.248 ms), completed the 21
fill/deny iterations and navigation-race case, and reached the blocking-race
case before timing out. Total extension case duration was 20.60 seconds.
This narrows this occurrence to a later operation, not initial native pairing.
It does not yet separate the block request, field comparison or unblock request.

## Read-only post-failure observation

The next test-only observer splits block-request/verification/unblock/return
phases and records whether the failed evaluation awaited a Promise. After a
`Runtime.evaluate` timeout only, it sends one fixed `true` evaluation in the same
owned session, with a separate two-second diagnostic bound and 128-frame ceiling.
It records only a closed responsiveness/error category, checks the exact request
and session IDs, and ignores late replies to the original command. The original
ten-second failure is then propagated unconditionally. There is no fill,
navigation, permission change, expression replay, stack sampling or payload log.

CDP's [evaluation contract](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/#method-evaluate)
distinguishes synchronous evaluation from awaiting a returned Promise; the
extension options helper uses the latter for messages to the worker. A responsive
post-deadline probe is evidence only at observation time, not proof that the
renderer was responsive throughout the failed command or that the worker caused
the delay. No runtime fix or browser-reliability closure is claimed yet.

Five browser-helper regressions passed, including real WebSocket correlation,
wrong-session/payload refusal and proof that a responsive probe plus a late
original reply cannot rescue the failed command. The phase observer also passed
its bounded-state test; the 74-input product architecture baseline still matches.

The refined run passed round 1, then **failed in round 2** at a different
boundary: the existing ten-second MCP tool-call timeout, phase `Discovery`,
iteration `0`. Both profiles had connected (1.551 s / 1.123 s); no fill in that
round had been requested. The extension case ended after 14.34 seconds. All 13
installed-client cases and installed CDP passed in both rounds. No round 3 or
successful cleanup followed. Because this was an MCP timeout, not a CDP
evaluation timeout, the renderer probe did not run; no responsiveness result
can be inferred. This occurrence must not be relabeled as the earlier options
or startup timeout.

## Chrome-only control

**PASS: all ten independent trials, 24.77 seconds total.** Each trial used two
fresh Chrome profiles, 65 background tabs, 21 page-local boolean comparisons
in each profile and navigation. The extension, native host, broker and custody
were absent. No background-throttling or security-policy bypass flags were added.
The control stops at its first failure rather than retrying a failed trial.

The separate, ignored `chromium_control` target keeps this experiment outside
the existing five-case `make test-browser-native` lane. Both targets compiled;
test enumeration confirmed five normal browser cases and one independent control.
The control ran before that file-only relocation; it was not rerun for a second
green result. Reproduce explicitly with fresh browser profiles:

```bash
MAGICVAULT_CHROME='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  cargo test --locked -p magicvault-effect --test chromium_control \
  -- --ignored --nocapture --test-threads=1
```

Set `CARGO_TARGET_DIR` and `MAGICVAULT_BROWSER_TMPDIR` to the intended disposable
build/profile volume first. This control does not exercise extension APIs or
native transport and has less work than the full case. Its pass narrows the
baseline observation, not the cause or browser-reliability gate.

## Independent MCP/daemon health observations

A further test-only observer separates initial and tab-narrowed discovery.
After the unchanged ten-second MCP timeout, it concurrently requests MCP tool
listing and direct daemon status, each with a separate two-second bound. Only
`Ready`, `NotReady` or `TimedOut` survives; no tool descriptions, daemon metadata,
errors or request arguments are logged. The timed-out operation always fails.
Neither probe retries discovery, fill, connection or permission changes.

All three phase/label/health regressions passed (2.00 seconds), including refusal
to echo unknown method labels and the health probe's own deadline.

That run passed rounds 1–6, then **failed in round 7**, phase `FillSettlement`,
iteration `3`, at the outer ten-second settlement deadline (33.75 seconds total
for the extension case). Both profiles had connected (779.634 ms / 373.870 ms).
All seven rounds passed the 13 installed-client cases and installed CDP. No
round 8 or successful cleanup followed. The per-MCP-call observer did not run:
the outer deadline can expire while polling, so it must not be treated as a
specific MCP tool timeout or proof of a stalled daemon.

## Scenario-wide failure observation

The final test-only observer covers failures after MCP setup, including the
outer fill deadline. It preserves the browser/MCP owners, catches the scenario's
unwind, observes bounded responsiveness and resumes the same panic before normal
cleanup. MCP tool listing and daemon status run alongside a fixed `true` primary
page probe and both profiles' fixed extension status messages. Status messages
project only a connected boolean inside the options page; no browser handles,
capabilities, content or effects are read or replayed. Primary page/status probes
are sequential on their owned socket (at most four seconds); the other probes
run concurrently, each bounded to two seconds. Startup failures before this
scenario retain the earlier setup checkpoints and command-level observation.

All eight browser-helper regressions passed (10.00 seconds), including fixed
extension-status requests, false/invalid results, silent-peer deadlines, the
128-frame ceiling and refusal to accept a late command as success. The three
phase/health regressions passed again. Observations are after a deadline, never
evidence of responsiveness throughout the failed operation.

**PASS: the final ten-round installed-candidate run completed all ten rounds.**
Each round passed the 13 installed-client cases, installed CLI → real CDP and
the complete two-profile extension case, including 20 fills, one denial,
navigation/block races and pause/reconnect. The driver then passed npm package
removal, stable native-path verification and recoverable app-only retirement.
The vault/keychain-preservation checks passed; no live custody or services were
touched. Idle, resource, long-soak and process-diagnostic lanes were not selected.

Neither a command timeout nor a scenario failure occurred in that final run,
so the new post-failure observations **did not execute against a real failure**.
This is not evidence that MCP, the daemon or either renderer remained responsive
in any earlier failed trial. The last observer only improves the next failure's
visibility; it is not a product fix or grounds to discard the retained failures.

## Outcome and next diagnostic boundary

Final fixture review extracted failure propagation for direct coverage: five
phase/health/propagation tests passed (2.00 seconds), including proof that a
successful scenario never polls failure observations and that observation
preserves the exact original panic allocation. This test-only extraction followed
the ten-round run; it was validated with those targeted tests and all-target
compilation, not another browser batch. Together with the eight browser-helper
regressions, 13 targeted observer tests passed. `cargo check --locked --workspace
--all-targets` passed; the 74-input architecture baseline and `git diff --check`
passed. All 120 relative file links across the seven edited documentation files
resolved. No new full-suite or remote-CI result is implied.

The browser release gate remains **OPEN**. Failures are no longer described only
as startup: distinct later evaluation, discovery and settlement deadlines were
observed. Their exact causes, and whether they share a cause, remain unknown.
No changed deadline, additional Chrome bypass flag, replay, publication or
signing was used. The successful app was recoverably retired; failed installs
and private logs remain separate evidence outside Git.

The next useful boundary is extension Chrome-API/native-message progress if
these bounded health probes identify a responsive MCP/daemon/page with a stalled
extension operation. Such instrumentation must remain synthetic and value-free;
modifying extension code would make that trial diagnostic, not qualification of
the unchanged downloaded assets. Do not repeat batches merely to obtain green
results, or claim an OS/browser/vendor defect without identifying evidence.
