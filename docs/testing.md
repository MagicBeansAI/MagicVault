# Coverage and qualification

## Current evidence

**Builds and automated suites pass on macOS arm64. Real headed/headless Chrome,
CLI-to-daemon-to-Chrome, and opt-in public-page smoke cases also pass on the
recorded Chrome version.** See the [current qualification results](qualification/results-2026-09-07.md)
for counts, commands, environments and limits. Real-browser cases stay ignored
in the default suite and run through explicit targets. The
[earlier verification record](phase3-verification-2026-09-07.md) retains the initial
build/full-suite and release-mode logging-safeguard evidence.

No installed-extension/native-human/keychain workflow, benchmark,
coverage-percentage tool or CI ran. Real CLI qualification uses test-only human
and key providers. Browser success does not establish measured performance,
all consumer regressions, broader browser compatibility or production readiness.
The [qualification index](qualification/README.md) separates executable
conformance tests, manual acceptance runbooks and future process gates.

Earlier [foundation results](targeted-tests-2026-09-06.md) and
[durable-audit results](deep-review-2026-09-07.md) belong to their recorded
revisions. They are not passing evidence for current 0.3.x standalone code.

## Focused lanes, when execution is authorized

The Makefile exports `CARGO_TARGET_DIR`. Set it explicitly when using a shared
external build volume, so artifacts stay in one intended location.

| Lane | Scope | Real browser / native service? |
| --- | --- | --- |
| `make test-compatibility` | Shared custody/metadata/audit and primitive compatibility fixtures | No |
| `make test-foundation` | Protocol, custody service, retained broker/storage/encoder unit cases, CLI and official SDK MCP integration | Synthetic local IPC/subprocesses only |
| `make test-browser` | Closed fill schema, CDP peer, native bridge, daemon effects, real CLI/MCP/native-host subprocesses, extension JS fixtures | Synthetic local transports/human/key providers only |
| `make test-browser-native` | Headed/headless fills, controls, frames and stale documents against disposable Chrome | Yes, explicit executable required |
| `make test-cli-native` | Shipped CLI → real daemon/IPC → real Chrome, allowed and denied fills | Yes; test-only human/key providers |
| `make test-public-web` | Example Domain navigation and Selenium public test-form fill, no submit | Yes; separate explicit public-network opt-in |
| `make test-qualification-fixtures` | Loopback fixture server and real child-process destination contracts | No browser; not a process product test |
| [Installed-extension runbook](qualification/extension.md) | Real permissions, native messaging, dialogs and browser input behavior | Yes; manual, disposable OS account recommended |

`make check` and `make test` are full workspace lanes, not prerequisites that
should be run when only targeted work is authorized. No automated workflow is
dispatched by setup. Dependency-only `make sync-lockfile` resolves the lock without
compiling or running tests; its execution, if any, is recorded in the review ledger.

The JS fixtures use Node's built-in test runner and VM with synthetic browser/DOM
objects, without a third-party DOM package. They cover the actual fixed fill
function and worker dispatcher, but are not real Chromium conformance evidence.
The Rust transport fixtures use `magicvault-test-support`, a non-production,
unpublished workspace helper. Its opt-in browser module also owns disposable
Chrome children and fixed local pages; generic test-only CDP helpers are not
exported by a production model tool.

## Coverage map

| Concern | Added or retained source |
| --- | --- |
| No values, human decisions, generic JS or unknown fields in agent requests | `magicvault-protocol/tests/browser_contract.rs`, retained `contract.rs` |
| Loopback endpoint, exact origins and safe metadata projection | `magicvault-effect/tests/cdp_transport.rs` |
| System-unique context, stale document, lost/malformed response, no replay | CDP transport fixtures and ignored `chromium.rs` |
| Native material/reply separation, initialization, IDs, frame caps and disconnect | `magicvault-effect/tests/native_bridge.rs` |
| Metadata is not effect permission; actual-use consent; caller/origin/expiry binding | `magicvault-service/src/broker/browser/tests.rs` |
| Single-use handles, spent-ID retention, partial/uncertain outcomes and typed audit | Broker browser fixtures |
| Post-effect audit failure blocks new work but permits status reconciliation | Broker browser fixtures |
| Maximum-size browser consent text fits the bounded native prompt | Broker browser fixtures |
| Shipped CLI → IPC → broker → CDP | `magicvault/tests/cli_flow.rs` |
| Shipped SDK MCP → IPC → broker → CDP and closed tool catalog | `magicvault-mcp/tests/end_to_end.rs` |
| Dependency payload logging compiled out of shipped executables | CLI/MCP `STATIC_MAX_LEVEL` assertions, intended for both debug and release qualification |
| Shipped native host → authenticated bridge → broker → trusted extension peer | `magicvault/tests/native_host.rs` |
| Native/reactive inputs, event delivery, opaque origins, ambiguity, disabled/changed controls and partial writes | `extension/tests/fill.test.cjs` |
| Site permission, document-targeted isolated execution, no page command route, disconnect | `extension/tests/worker.test.cjs` |
| Headed/headless real DOM delivery and independent browser-tool continuation | Ignored `magicvault-effect/tests/chromium.rs` |
| Actual CLI/daemon/Chrome, allow/deny and value-free output/audit | Ignored `magicvault/tests/browser_native.rs` |
| Explicit public-page navigation and synthetic fill without submit | Ignored `magicvault-effect/tests/chromium_public.rs` |
| Reusable local pages and child env/stdin/IPC/echo contracts | `scripts/tests/qualification-fixtures.test.mjs`; process effects remain unimplemented |
| Key/store identity, scoped facade and durability compatibility | Retained shared-library lanes; consumer-owned integration tests remain with the consumer |

The dated record identifies which fixtures passed. Ignored browser cases were
executed through opt-in lanes; genuine native manual gates remain unexecuted.
This map is not a numeric coverage percentage or a promise that
unmeasured paths work. Report omissions and execution failures honestly.

## Real CDP qualification

Set `MAGICVAULT_CHROME` to an absolute, trusted Chrome/Chromium executable and run
the explicit native lane only when authorized. The fixtures create a new temporary
profile, bind a loopback HTTP fixture and debugging port, use synthetic values,
and clean up their own child/browser on completion. They do not use a personal
profile, default user data directory or live credential store. Headed qualification
requires an interactive graphical session.

Use the exact commands, reusable fixture pages and public-site safety constraints
in the [browser runbook](qualification/browser.md), plus the
[actual CLI and native-human runbook](qualification/cli.md).

Record exact OS/browser/compiler/dependency revisions, headed versus headless
mode, and any unsupported frame/control combinations. Do not extrapolate to
other Chromium brands, legacy headless shells or headless-extension combinations.

## Installed extension and native host qualification

Prefer a disposable OS account: native-host definitions and keychain are OS-user
resources, not isolated by a Chrome profile. Use a fresh standalone root, synthetic
enrollment and a disposable browser profile. Follow the
[installed-extension runbook](qualification/extension.md), recording:

1. Build and source packaging, unpacked installation, exact extension ID,
   generated host definitions, native prompt/keychain behavior and connection.
2. No site permission: target omission and explicit refusal, with no value sent
   to an unpermitted document. Grant/revoke site permission and try again deliberately.
3. Main document and explicit permitted frame fills, native and reactive fields,
   and continued navigation/submission through the original browser tool.
4. Wrong or opaque security origin, top-page/frame mismatch, disabled fieldsets,
   unsupported/changed controls, duplicate matches, navigation during consent,
   document replacement and target closure. Inspect maximum-size native consent
   rendering; no hidden/truncated destination or field selection may be approved.
5. Denial, cancellation, host/worker/daemon disconnect, reconnect with new handles,
   unknown status after restart and no implicit repeat of an uncertain operation.
6. Own-channel canaries: inspect value-free operation replies and sanitized
   diagnostics/typed audit; never upload live profiles, full traces or vault files.
7. Remove only the managed extension/host definitions, refuse modified/foreign
   files, leave vault/keys/pairings intact, and reconnect after an intentional reinstall.

These manual/native gates remain unexecuted. They are required before claiming
the installed extension path is qualified, not replaced by a fake native peer.

## Performance, recovery and consumer gates

Source safeguards include bounded messages/contexts/targets/jobs, one connection
per registered CDP browser, isolated-world reuse per connection/document,
non-queuing adapter admission, no browser I/O under the custody lock, and no human waits
under that lock. Selected credential entries are cloned once per request rather
than once per field, and temporary owned values are zeroized on drop.

Actual latency, idle CPU/memory, peak allocation, repeated-fill stability,
contention and shutdown measurements remain outstanding. Synthetic audit-failure
propagation passes; repeated app installs, crash/recovery, power-loss durability
and rollback drills remain unexecuted.
Qualification must retain the core compatibility lane and relevant consumer-owned
tests against any changed shared dependency; unchanged shared source is evidence
of scope, not a substitute for all consumer runtime tests.
