# Second deep Phase 1/2 review

Review baseline: `5b99781e2fc4e4e1138aaacfe78627f839199837` (private repository).
Core advances to `0.1.3`; service/CLI/MCP to `0.2.2`. Primitives `0.1.1`, protocol
`0.2.0` and wire version `1` are unchanged. No publication or live deployment.

## Important defects fixed

1. **Standalone audit success did not establish durability.** Broker decisions
   called the legacy `try_audit_event`, which only wrote journal bytes. A later
   power loss could lose audit evidence for an acknowledged, persisted decision;
   sync failures could not trigger the broker's fail-closed path. The new opt-in
   `try_audit_event_durably` appends and syncs the file, its directory, and the
   parent naming that directory under the existing journal lock. The broker uses
   it and poisons on any failure. Its instance root was already durably created.
   Failure is potentially post-append and is never automatically retried.
2. **Startup omitted validation of the existing audit journal.** The standalone
   vault guard checked encrypted files but not the journal. The legacy core
   writer could follow a pre-existing journal symlink and append/chmod a target
   outside the instance. Startup now requires an owned, private regular journal
   before handing paths to core. It does not follow, repair or delete unsafe
   entries. This guards imported/accidental unsafe state; it is not isolation
   against unrestricted same-user code replacing files after validation.

## Regression and performance boundary

Existing `audit_event` / `try_audit_event` behavior, journal schema and optional
typed receipts remain unchanged. The private append helper uses a monomorphized
completion callback; legacy callers do not execute new filesystem syncs. Only
standalone decision paths pay the durability barriers, on their existing blocking
writer. No new lock, writer, plaintext projection, runtime owner or IPC hop is
introduced into Magician. Its existing recovery policy still tightens a loose
legacy journal on append; standalone's stricter startup policy is separate.

The review traced IPC admission/framing/UID and capability authentication,
epochs/lost replies, broker authorization and replay, enrollment expiry,
persistence/shutdown ownership, native child cancellation, CLI/MCP output and
LaunchAgent ownership. Phase 1 comparison covered scope/cache facades, audit
receipt identity, source attestations, features and moved source boundaries.
It did not re-audit every unchanged runtime algorithm or establish no defects.

## Executed targeted evidence

`CARGO_TARGET_DIR=/Volumes/ssd1/magician/builds/wt/magicvault-extraction/extracted/magicvault
RUSTC_WRAPPER= DOCS_HOOK_DISABLE=1 make -C .extracted/MagicVault
test-compatibility test-foundation` passed from the Magician worktree.

| Selected scope | Passed |
| --- | ---: |
| Core old/new vault and typed/durable audit wire integration | 4 |
| Value-free metadata projection integration | 1 |
| Core audit-only unit cases | 2 |
| Shared primitive unit/integration cases | 24 |
| Standalone protocol/service/CLI/MCP foundation | 27 |
| Total, excluding reruns | 58 |

Four new tests cover legacy/durable journal interoperability, failure after append
while retaining the journal lock (private injected finalizer), startup refusal of
symlink/non-private journals without touching the outside fixture target, and
broker poisoning before pairing publication on an audit append failure. The
existing metadata-projection test is now included in the narrow compatibility
lane rather than only the full workspace lane.

Real CLI/MCP subprocesses and Unix IPC used temporary synthetic human/key
providers. No native dialog, keychain, installed daemon, LaunchAgent, live vault,
provider, benchmark, full suite or CI ran. Ordinary fsync completion and injected
error propagation are not a crash/power-loss qualification. Product integration
results are recorded separately in Magician's review ledger.
