# Phase 3 implementation and static review ledger

This preserves the original no-execution implementation checkpoint. A subsequent
authorized round passed checks, optimized builds and targeted/full-suite tests;
see the [dated verification follow-up](phase3-verification-2026-09-07.md).
Subsequent [real-world qualification](qualification/results-2026-09-07.md) records
195 distinct passing cases, including disposable Chrome and CLI integration.
Those results do not retroactively turn this original static review into
executed evidence or qualify installed-extension/native-human/keychain acceptance.

Status: source implementation and static-review checkpoint complete, 2026-09-07.
No checks, builds, tests,
benchmarks, coverage tools, real browser/native operations or CI have run in this
round. The [plan](phase3-browser-plan.md) and [coverage map](testing.md) define
the source-completion and later qualification boundaries.

## Implementation record

| Component | Source path / responsibility |
| --- | --- |
| Browser protocol | `magicvault-protocol/src/browser.rs`; closed requests/statuses, v2 standalone envelope |
| Trusted effects | `magicvault-effect`; dedicated CDP, bounded native bridge, shared fixed fill function |
| Authorization and jobs | `magicvault-service/src/broker/browser.rs`; explicit permissions, document-bound one-use consent, lifetime and typed audit |
| Human/native service | Existing native consent plus `native.rs`, private `bridge.sock`, exact profile/extension configuration |
| Executables | CLI, official-SDK MCP and `magicvault-native-host`; actual daemon/adapter paths |
| Extension | Manifest, worker, fixed-function packaging, setup/permission UI; no website-accessible credential API |
| Public handoff | README, changelog, security, setup, browser usage, builder protocol and qualification docs |

Standalone protocol/service/effect/CLI/MCP use `0.3.0`; agent protocol is v2 and
native bridge wire is v1. Core `0.1.3` and primitives `0.1.1` remain untouched.
No Magician or MagicRun repository, live store, keychain, service or profile is
changed by this implementation. Nothing has been committed, pushed or installed.

## Findings addressed during implementation

1. **Implicit authority expansion:** inherited enrollment permits metadata only.
   Added explicit per-client field/exact-origin browser rules with native consent,
   then separate consent for each exact fill. Did not widen core policy or
   rewrite existing credential entries on upgrade.
2. **Wrong target after navigation:** backend IDs and origin alone are insufficient.
   Bind top/selected document identity and lifetime; use CDP system-unique isolated
   execution contexts or extension document IDs, and revalidate before delivery.
3. **Replay after result eviction:** detailed job expiry could reopen an old
   operation under a replacement handle. Consume target handles at admission and
   retain bounded spent-ID tombstones for the daemon epoch. Never retry effects.
4. **Misleading success after partial effects:** field writes, site handlers,
   transport disconnect and audit failure are not transactional rollback. Preserve
   per-field status; report partial/uncertain separately from complete delivery.
5. **Unqueryable audit uncertainty:** normal readiness refusal also blocked
   reconciliation. Allow only authenticated read-only fill status after faulting;
   new mutations remain refused. Record typed, value-free completion receipts.
6. **Native reconnect and initialization races:** gate bridge commands until the
   ready handshake is sent; approved reconnect replaces the old extension channel
   and invalidates old handles/jobs. No background replay or page-message route.
7. **Browser diagnostic reflection/panic:** closed CDP/native outcomes discard raw
   exceptions and events. Replaced mutable JSON indexing of untrusted reply shapes
   with checked access so a malformed scalar cannot panic a fill job.
8. **Unbounded or avoidable allocations:** bound websocket/frame/context/event
   sizes and counts; reuse an isolated world per connection/document; resolve each
   selected custody entry once rather than clone it again for every field.
9. **Blocking input/shutdown paths:** reject non-regular CLI request files using a
   nonblocking, no-follow open; wait for the custody writer on the blocking lane
   during shutdown; bound native-host stdio teardown without owning custody writes.
10. **Ambiguous same-origin tabs:** public discovery now includes safe backend
    tab/frame IDs for explicit mapping, rather than indistinguishable origin rows.
11. **Installer ownership and exposure:** exact allowlisted extension ID; private
    config/wrapper; no capability in wrapper/manifest; exclusive publication and
    content-validated removal without deleting credentials or foreign definitions.
12. **Package/tool regressions:** advance standalone versions together, keep the
    default CLI binary selection, update the explicit MCP catalog and retained
    fixture expectations, and document fail-closed standalone downgrade behavior.
13. **Transitive transport payload logging:** inspection of the selected
    websocket implementation found raw message/frame debug/trace paths. Shipped
    executables now enable `log`'s `max_level_off` and `release_max_level_off`;
    added CLI/MCP assertions for the compile-time bound. Reusable core/effect
    crates do not globally suppress a host's logs. Their documented integration
    contract requires permanent payload-log suppression by the trusted host.
14. **Late cancellation and oversized consent:** recheck cancellation/deadline
    after custody resolution and immediately before material release; preserve
    cancelled versus expired status. Bounded valid browser requests can produce
    more than 4096 bytes of escaped consent metadata, so the prompt cap is now
    16 KiB, with worst-case rendering-source fixtures. Secret input stays at
    4096 bytes. Real native dialog visibility still requires qualification.
15. **URL origin is not document authority:** the shared isolated-world function
    now checks the global security origin as well as location origin, refusing
    opaque documents. The distinction is defined by the
    [HTML global-origin contract](https://html.spec.whatwg.org/multipage/webappapis.html#dom-origin).
    Added synthetic opaque-origin coverage; actual browser behavior is not yet
    qualification evidence.
16. **Controls changing during a multi-field fill:** revalidate type, visibility,
    inherited disabled state and length constraints before each write, not just
    at initial selector resolution. Added disabled-fieldset and event-driven
    mutation fixtures that preserve partial outcomes without filling later fields.
17. **Native invocation normalization:** accept only the configured extension's
    exact origin with or without a single trailing slash. Paths, queries,
    fragments and other extension IDs remain refused; manifest origins remain
    canonical. Added identity/configuration coverage.

## Actual tool activity and final scope review

- Read-only source, manifest, documentation and Git-diff inspection; official
  CDP/Chrome/HTML API references; no browser control or live configuration reads.
- Dependency-only offline `make sync-lockfile` (`cargo update --workspace`),
  using a temporary external artifact target. It added the websocket dependency
  graph and standalone workspace entries; existing registry package versions
  and shared core/primitives lock entries were not upgraded. A later resolution
  after adding executable logging safeguards locked zero additional packages.
- Formatting-only `rustfmt` on changed standalone Rust files. No check mode,
  compiler, build, test runner, linter, benchmark or coverage invocation.
- Final static tracing covered reference request → authenticated daemon → native
  consent/policy → custody resolution → CDP or native host/extension → closed
  outcome → durable receipt → CLI/MCP status, including shutdown/reconnect paths.
- Read-only diffs confirmed no changes under `magicvault-core` or
  `magicvault-primitives`. No edits in Magician/MagicRun, staging, commits, pushes,
  installation, service changes or CI dispatch occurred.

The public README, security boundary, changelog, usage/setup, builder contract,
capability/roadmap table, source fixtures and focused execution lanes are present.
Historical review records retain their results; machine-specific build paths and
outdated present-tense repository-privacy claims were generalized or contextualized.
This review does not prove the absence of remaining important defects.

## Deliberate support limits

The initial CDP adapter addresses page sessions and supported frames. Inaccessible
out-of-process frames, opaque origins and unsupported controls fail closed, not
through a numeric-context or arbitrary-JS fallback. Extension frame delivery uses
document-targeted execution plus explicit site permission. Broader combinations
require actual platform qualification.

The native installer initially supports one configured extension/profile per user
for its host name. Registered browsers and jobs are ephemeral; restart/reconnect
requires explicit rebinding. All fills use genuine native consent and serialize
human interactions through completion; no unattended policy mode is introduced.

The [security boundary](../SECURITY.md) explicitly excludes other browser tools'
observation paths, arbitrary same-user/privileged software and recipient copying.
Neither output masking nor the adapter's name broadens that promise.

## Evidence outstanding at the original checkpoint

All new fixtures are added, not executed. Compilation, browser/native functional
acceptance, measured performance, coverage percentage, crash recovery and
consumer regression qualification remain unproven. No full suite is authorized.
Any later results must identify exact revisions and not rewrite this checkpoint
as if it had been tested.

The follow-up records supply compilation, automated suites and scoped real-CDP/CLI
evidence. Installed-extension/native-human/keychain, measured performance and
broader consumer qualification remain open. Statements above about no execution,
commits or installs apply to the original static-review checkpoint only.
