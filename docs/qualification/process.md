# Process and running-service qualification contract

**MagicVault does not yet implement `secure_new_process` or `secure_new_http`.**
The current standalone delivery surfaces support browsers. These fixtures prepare future
integration with MagicRun; they do not implement or qualify a process credential
delivery feature, and do not require changes to MagicRun now.

## Runnable recipient fixtures today

```sh
make test-qualification-fixtures
```

The test runner launches real Node child processes with synthetic data and tests
`test-support/process-fixtures/child.mjs`:

| Mode | Recipient behavior | Future integration purpose |
| --- | --- | --- |
| `env` | Compare a fixed synthetic environment variable; emit a boolean only | New-process environment delivery without model/argv exposure |
| `stdin` | Bounded stdin, compare the synthetic canary, emit a boolean | Pipe delivery, EOF, cancellation and input lifetime |
| `echo` | Intentionally emit the public canary on stdout and stderr | Prove future output mediation blocks recipient echo in both streams |
| `stateful` | Bounded JSON-lines `set`/`status`/`shutdown`; acknowledge rotation without echo | Explicit credential-provider/IPC updates to an already-running service |

The echo mode is deliberately unsafe **as an agent output** and uses only a
hard-coded public fake value. Its passing fixture test proves that the adversarial
probe works, not that MagicVault filters it. Never supply real credentials to it.
Fixture subprocesses have timeouts and are killed/reaped by the owning runner.

## Required gates for a process-delivery integration

| Case | Required end-to-end property | Current status |
| --- | --- | --- |
| P-01 authority | Reference-only CLI/MCP request → exact executable/argv/cwd/destination-bound consent → MagicRun execution | NOT IMPLEMENTED |
| P-02 delivery | Approved values reach only selected child env/stdin/provider slots; no secret CLI arguments | NOT IMPLEMENTED |
| P-03 denial/races | Deny, cancel, expiry or revocation before dispatch causes no child material delivery | NOT IMPLEMENTED |
| P-04 hostile output | Echo/encoding/error/partial UTF-8/chunk boundaries cannot expose canaries through stdout, stderr, logs, status or audit | NOT IMPLEMENTED |
| P-05 lifecycle | Exit/signal/timeout, pipe backpressure, input caps, descendant ownership and cleanup are bounded; never kill unrelated PIDs | NOT IMPLEMENTED |
| P-06 uncertainty | Lost reply or partial launch does not automatically launch a second credential-bearing child | NOT IMPLEMENTED |
| P-07 running service | Explicit authenticated provider/IPC contract supports rotation/revocation; existing service PID/lifecycle stays owned by its host | NOT IMPLEMENTED |
| P-08 regression | MagicRun and Magician's existing execution/credential paths pass consumer-owned regression tests | Required when those repos actually change |

An already-running process cannot be treated like a new child environment.
Use an application-supported provider, IPC, reload or refresh seam with explicit
authority; do not promise universal injection into arbitrary processes/PIDs.
The stateful fixture demonstrates that cooperative seam only.

When implementation begins, add an integration that drives these fixtures through
the **actual public MagicVault CLI/MCP → broker → MagicRun path**, not a test that
directly spawns the recipient and claims product success. Keep new HTTP request
qualification separate, with destination/redirect/header and response-mediation
tests against a local synthetic HTTP server. Public provider calls and real
credentials are unnecessary for deterministic acceptance.
