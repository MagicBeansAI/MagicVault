# Targeted migration tests — 2026-09-06

After the static-review checkpoint, the owner authorized migration-focused tests
only, never the full suite. No full-workspace test/check, benchmark, CI workflow,
native keychain/dialog or installed-service operation was run.

## Executed scope and findings

At baseline `ecdcbdc638575c50795dfd8f9fb4d1769b35a980`,
`make test-compatibility` passed: three core extraction wire/audit tests and
24 primitive tests (20 unit, four durable-I/O integration). These exercise
baseline/candidate vault compatibility, stack-safe JSON and private staging.

The first `make test-foundation` exposed issues missed by static review:

1. Production MCP response encoding used the old `Content` name. The pinned
   `rmcp 3.1.0` API uses `ContentBlock`; corrected without changing wire output.
2. The MCP integration fixture expected `CallToolResponse` from the SDK's
   continuation-driving `call_tool`, which returns `CallToolResult`. It now
   calls `call_tool_once`, preserving the assertion that no continuation/task
   response is allowed on these foundation paths.
3. Standalone test roots incorrectly assumed `tempfile::tempdir()` was private.
   Its default directory mode follows umask and can be `0755`. Fixture roots
   now request `0700` at creation; production ownership/mode refusal is unchanged.
   The existing unsafe-root negative test remains enabled.

After these narrow fixes, `make test-foundation` passed all 25 tests:

| Package/path | Passed |
| --- | ---: |
| CLI subprocess flow and rejected-input output | 2 |
| MCP encoder bound, blocking-stdio teardown, catalog and actual SDK/subprocess flow | 5 |
| Protocol value/grant rejection and metadata bounds | 2 |
| Service deadline/capacity/initialization unit cases | 5 |
| Service IPC, consent/replay/revoke, restart, uncertain persistence and startup guards | 11 |

Tests used temporary roots, synthetic values and fixture human/key providers.
Actual CLI/MCP subprocesses and Unix IPC ran; real Apple dialogs, keychain
enrollment and LaunchAgent loading did not. Build outputs were isolated beneath
the consumer worktree's Makefile-exported build directory. No full test lane
was selected. Magician's separate integration results belong to its repository.

## Still not qualified

These targeted results do not establish a full regression pass, performance or
coverage percentage, crash/power-loss guarantees, native host acceptance or
release readiness. The [static ledger](static-review-2026-09-06.md) is historical:
its no-tests statement describes that checkpoint, before this authorization.
Repository visibility and the model-channel trust boundary remain unchanged.
