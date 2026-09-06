# Phase 1 / Phase 2 follow-up static review

Review baseline: `7a5e306e0872a0a0fcdfef90513d04b91a1eaa68`.
Scope: extracted custody/persistence interfaces and their Magician facades;
standalone storage, broker, native-human, IPC/client, CLI and SDK MCP paths;
dependency/test ownership. This is source review, not a penetration test,
runtime qualification, or an exhaustive audit of all unchanged product code.

No checks, builds, tests, benchmarks, native dialogs, keychain/service operations,
formatters, docs guards or CI were run. Only source/Git inspection and lockfile
dependency resolution were performed. Repositories remain private.

## Findings fixed

| Finding | Impact and fix | Added regression source, not executed |
| --- | --- | --- |
| Important: staging mode applied after writing | The extracted synchronous helper inherited `File::create` → write/fsync → chmod. Under a permissive umask and traversable parent, a private payload could be readable through its temporary name before chmod. Exclusive creation now supplies the requested Unix mode; exact descriptor permissions precede bytes and fsync. Existing paths/symlinks are not adopted or cleaned up. Magician's async twin receives the same fix in its repository. | Primitive unit test inspects permissions before the first byte and refuses existing files/symlinks; integration test verifies exact replacement mode. Magician adds equivalent async staging, replacement and failed-publication tests. |
| Correctness: bare relative filename falsely fails after commit | Its parent is the empty path, so parent sync failed after successful rename. Empty parent now means `.`; parent creation skips the empty path. | Primitive integration case exercises bare-filename parent sync without changing process CWD; full replacement/cleanup fixtures retained. |
| Important: approval replay check and reservation raced | Two calls could pass the old optimistic lookup before reservation. A replay could get `busy`, or overwrite an earlier completed request and open another prompt. Binding lookup, nonblocking human admission and reservation now share one serialized transaction. Completion checks owner/reference again. | Sixteen concurrent same-ID broker calls must return the original pending job; changed owner/reference must conflict. Existing native-cancellation fixture controls the human wait. |
| Important: enrollment deadline was not checked at persistence | Blocking-pool/state-lock/audit delay after the last pre-transaction check could allow enrollment after its 180-second window. An absolute monotonic deadline is revalidated inside the transaction and after audit immediately before the core write. | Expired/live deadline helper cases plus existing enrollment, denial and cancellation coverage. No clock-injected audit-delay integration case or measured deadline latency is claimed. |
| Important: blocked stdio could hang MCP process shutdown | Tokio's stdin/stdout use blocking tasks that may outlive SDK transport deadlines. Ordinary runtime drop waits for them indefinitely. Only the MCP client uses bounded runtime teardown (250 ms after SDK completion); process exit then ends remaining stdio threads. It owns no durable commits. The custody daemon still drains all commits and retains its lease. | Synthetic uninterruptible blocking-I/O task verifies teardown returns before its input is released; actual SDK/subprocess fixtures retained. |
| Important: initialization acknowledged a root without syncing its parent entry | Syncing files and the root itself does not persist the parent's entry naming a newly created root. Initialization now syncs that parent under the lease before key creation, including retry after an uncertain empty setup. | Private key-backend seam tests failed key creation, absent identity, safe retry, stable existing identity/modes and refusal to adopt unrelated state. No executable flag or public fake-backend selector was added. |

No actual disclosure or crash was reproduced. The staging issue is a source-
identified permission window, not evidence that the private standalone root
was readable. Existing trusted-parent assumptions and the unrestricted-same-user
threat-model exclusion remain; file modes do not isolate hostile same-user code.

## Integration and performance

Versions: primitives `0.1.1`, core `0.1.2`, service/CLI/MCP `0.2.1`.
Protocol crate stays `0.2.0`, wire version stays `1`. Core `0.1.2` carries the
minimum primitives version with the persistence fixes; core APIs, encryption,
partition filenames, policy, scope layouts and serde formats are unchanged.
Magician advances both Git dependencies together and keeps its async admission
and runtime owners. It does not adopt the standalone service or MCP teardown.

Exclusive staging does not add a material copy or another data fsync. Descriptor
permission changes replace the old chmod; admission still precedes async rename.
Approval reservation uses fewer serialized blocking dispatches for a new job;
human waiting remains outside locks. The additional root-parent sync is setup-
only. These are code-level observations, not measured performance results.

`make test-compatibility` now includes primitive unit/integration tests as well
as core extraction compatibility; consumer compatibility also includes Magician's
async durable writer. All existing foundation and product suites remain required.
No coverage percentage or pass result is available.

## Remaining acceptance

Compilation, automated suites, macOS UI/keychain/LaunchAgent behavior, real stdio
shutdown, crash/power-loss durability, throughput/latency, coverage measurement
and Magician regression/rollback evidence remain unrun. Static review cannot
establish absence of remaining bugs or the promise of zero regressions.
No browser, extension, HTTP, process or running-service effect is added here;
metadata consent is not delivery authority. See the [foundation ledger](phase2-foundation.md),
[protocol](protocol.md) and [setup boundaries](setup.md).
