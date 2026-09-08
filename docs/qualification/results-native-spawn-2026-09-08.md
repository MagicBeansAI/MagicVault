# Native macOS process launch correction — 2026-09-08

The [previous original-scenario recurrence](results-launch-stage-2026-09-08.md)
ended before the callback entry marker, with an owned `SIGKILL`-terminated child
and OS category `Foundation`. That category did not identify an exception site.
This change removes the affected userspace fork interval instead of adding
another observer or serializing the process/HTTP scenario.

## Revisions and scope

MagicVault CLI/MCP/service source versions are `0.8.3`; effect is `0.6.1`.
Cargo.lock selects MagicRun `0.1.74`, commit
`af348ab566cbf495f59d155a328bf2cac6afa09d`. Custody core `0.1.3`, extension
`0.6.1`, wire/storage versions, browser/HTTP behavior and native consent remain
unchanged. This is a source change and qualification candidate, not publication.

Rust's registered `pre_exec` callback selects its fork path. Apple
[recommends combined spawning when using higher-level frameworks](https://developer.apple.com/forums/thread/737464);
the [reviewed Darwin implementation](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/spawn/posix_spawn.c)
provides native spawn and a descriptor-based cwd action. This supports removing
the unsafe launch interval; it does not retrospectively identify the precise
exception in the earlier child.

Only **non-jailed macOS batch execution**, including standalone MagicVault
process delivery, changes backend. Jailed commands retain their existing
resource-limit hook; omitting it would weaken policy. PTY and non-macOS paths
are unchanged and are not newly claimed to be fork-free. Magician's checkout,
dependency, source approvals and live acceptance installation are untouched.

| Launch invariant | Native implementation |
| --- | --- |
| Executable and arguments | Original authorized bytes, absolute snapshot, no PATH search or shell fallback |
| Environment and material | Exact explicit environment, no inheritance; new C-string copies zeroize on drop; malformed/duplicate input refused |
| Working directory | Duplicate the opened authorized descriptor; apply native fchdir action, not a path lookup |
| Stdio and descriptors | Sources above 0–2 prevent action collisions; explicit dup/close and close-by-default; no parent descriptor closure |
| Signals and ownership | New process group at spawn; inherited mask and ordinary SIGPIPE reset; existing collector and cleanup |
| Failure and resources | No launch fallback/retry; exact owned PID, cached reap, interrupted-wait handling and unwind cleanup; existing deadlines, watchdog and output bounds |
| Compatibility | Public cwd action requires macOS 10.15+; missing support refuses the operation without a fork fallback |

All preparation happens before the runner's final cancellation/deadline check
and authority revalidation. The native path adds no helper executable, daemon,
service thread, background polling or event history. Its fixed preparation cost
is bounded by existing invocation limits; no performance improvement is claimed
without comparable measurements. A debug snapshot records `MacosPosixSpawn` with
no callback stage or shared probe, and the exact CLI fixture asserts that choice.

Existing macOS batch source-attestation bytes include the entire new backend,
so consumer upgrade reviews cannot accidentally omit it. Cwd authority source
also changes for the safe borrowed-descriptor accessor. A future Magician
upgrade must review the new fingerprints normally, never preserve an old hash.

## Static review and retained development finding

Review covered the exact inputs, C-buffer lifetimes/zeroization, action ordering,
stdio collisions, cwd substitution, opaque FFI owner destruction, PID ownership,
error handling, cached reaping, backend selection and unchanged collection path.
No production runtime option enables weaker behavior or bypasses native consent.

The first new native-backend test run had **seven passes and one failure**. Its
malformed-input case exposed an adapter error: reading argv back from `Command`
can lose an invalid-NUL indication retained privately by Rust. The adapter could
therefore accept substituted input. It was changed to accept the original
authorized bytes directly, and the negative cases now exercise that boundary.
No delivery trial was retried or deadline changed to address this finding.
The corrected eight-test run and subsequent versioned 25-test batch run passed.

## Local validation

macOS `26.6` arm64, Rust `1.92.0`; normal and diagnostic build outputs on SSD1.

- MagicRun `0.1.74` all-target check and 25 batch tests passed (eight new backend
  cases plus 17 existing batch regressions). Sixteen authority tests, eight PTY
  tests and the bundled-source attestation test passed during review.
- New real-process cases cover exact args/env/stdin/streams, cwd replacement
  after preparation, missing/invalid/denied launch, cached exit/signal status,
  bounded live-child drop, closed parent stdio, and an inheritable sentinel FD.
- A dedicated subprocess registers a fork handler: native launch observed zero
  invocations; the explicit fork-path positive control observed one. A completion
  marker prevents an incorrectly qualified zero-test helper from passing.
- Both diagnostic normal/nonzero/signal and deadline tests passed. The existing
  jailed fast-exit case passed with assertions retaining the standard backend;
  nine historical diagnostic-unit tests passed. These are not wider jail/PTY
  fork-safety qualification.
- The reviewed 52-input MagicRun architecture baseline and 11 gate tests passed.

MagicVault `0.8.3` all-target check, five process and eight HTTP tests passed
against the exact locked dependency. The five real adapter diagnostic cases
passed while asserting native-spawn selection, and the original CLI driver
compiled with the same assertion. Eighteen orchestration/packaging tests and
12 architecture tests passed. Previous CI failures and the earlier local timeout
remain in their original records; no full suite or live acceptance was rerun.

## Original-scenario CI

Pending one manual run on the reviewed MagicVault revision: uninstrumented
unsigned clients, instrumented synthetic broker, original concurrent process/HTTP
pair, at most 200 fresh trials within ten minutes, stop at first failure. No
failed-trial replay, broader crash collection, signing, publication or live
desktop modification is authorized by this qualification.
