# Installed browser permission and in-flight recovery qualification — 2026-09-08

Source: committed `34fee4a1c578b12235957f044a41808d7ee76dc1`, pushed to
`origin/main` before this acceptance session. CLI/MCP/service are `0.8.1`,
protocol/effect `0.6.0`, agent wire `4`, bundled extension `0.6.1` and native
handshake `2`. Installed artifact hashes and prior native recovery evidence are
in the [cancellation record](results-cancellation-2026-09-08.md).
No new runtime change or rebuild was made for these observations. The current
connection alone does not establish which explanatory extension copy Chrome
has loaded; the last human-confirmed worker version was `0.6.0`.

## Scope and safeguards

Only the explicitly authorized dedicated acceptance app/vault/client, its one
connected extension profile, local synthetic browser page and local test
recipients are in scope. Native approvals and permission choices remain human
controlled. GUI automation stays off during native entry; no credential values,
pairing capabilities, arbitrary page snapshots or unrelated browser tabs are
inspected. Discovery is narrowed to the exact fixture origin and selected tab.

Website access, credential destination policy and remembered use are distinct
controls. Removing Chrome's local HTTP permission affects that exact host across
ports, not just a URL path. Any permission change must be reported explicitly and
restored deliberately; no broader grant is silently added to obtain a pass.

## Browser acceptance

| Case | Status | Evidence |
| --- | --- | --- |
| Initial target discovery | **PASS** | Human opened the repository's local synthetic page; narrowed discovery returned exactly one matching main-frame target. No field value was read. |
| Credential destination setup | **PASS** | Initial reference-only fill request returned `denied` before a pending job/native use prompt. Human then confirmed Allow for the synthetic password field and exact fixture origin; CLI acknowledged `browser_credential_configured`. The remembered-use list remained empty. |
| Remembered-use baseline | **PASS** | Human confirmed Always allow; the same operation settled as `filled`, field `filled`, error `null`. One grant bound the paired client, extension/profile identity, exact top/frame fixture origins, main frame and synthetic password-to-`#password` mapping. No native prompt remained. This is receipt evidence, not an independent visual event-counter observation. |
| Website permission removal with remembered use | **PASS** | Human confirmed removing the fixture host's Chrome permission while leaving the tab open. A single prepared request against a still-valid pre-removal binding settled as `denied`, error `permission_denied`, field `not_filled` rather than an expiry/stale-target refusal. Fresh exact-origin/tab discovery returned no targets. The same exact-use grant remained unchanged and no native consent prompt remained. |
| Permission restoration and fresh-document remembered reuse | **PASS** | Human confirmed restoring the same local HTTP host permission and reloading the fixture. Narrowed discovery returned one fresh target and the exact-use grant remained unchanged. One new operation settled as `filled`, field `filled`, error `null`, without another human approval; no native prompt remained. The denied operation was not replayed. |
| Extension pause and old-binding refusal | **PASS** | Human confirmed Pause connection. The client's browser list became empty; one prepared request using the old binding returned `stale_target` before any fill. The exact-use grant remained unchanged. Doctor confirmed the same daemon epoch ready with verified app/native-host definitions and zero connected browser profiles. This does not qualify pause persistence across a browser restart. |
| Resume and fresh-handle remembered reuse | **PASS** | Human confirmed Retry / resume and fixture reload. The same extension profile reconnected with a new browser handle; the exact-use grant remained unchanged and no native prompt was pending. A separately discovered target and new operation settled as `filled`, field `filled`, error `null`, without another approval. No old operation was replayed. |
| Browser test-grant cleanup | **PASS** | Revocation of only this exact-use test grant was acknowledged; the remembered-use list became empty. Restored local website access, the running extension connection, credential destination rule and pairing were retained. |

## In-flight cancellation

Private synthetic recipients are prepared. The process destination was
registered after the human confirmed Allow; CLI returned a reference-only
profile, the remembered-use list stayed empty and no recipient start/PID/
heartbeat/completion file existed. Registration did not launch the process.
The human separately confirmed Allow for the HTTP destination registration;
CLI returned its reference-only profile and the grant list remained empty.
All HTTP recipient counters stayed zero, confirming registration sent no request.
The process records only a fixed start marker, PID and bounded heartbeat bytes; the
HTTP recipient records request/presence/active/close counters and holds a bounded
stream open. Their intentional credential echoes may only be consumed by
MagicVault's withholding boundary, never directly by the operator. Neither
recipient sends traffic to a public service or stores a credential in a file.

A one-shot private monitor submits a single reference-only operation through the
installed CLI, waits for recipient activity and a running receipt, then calls
explicit cancellation and reconciles that same ID. It never answers a native
prompt, reads process output/HTTP echo responses, or retries an effect. Status
and cleanup observations are bounded; unknown diagnostics fail the case and
request cancellation of only the submitted operation. Its syntax and invalid-
argument no-dispatch path were checked separately, not counted as product tests.

| Native case | Status | Evidence |
| --- | --- | --- |
| Process in-flight cancellation | **PASS** | Human confirmed the requested Allow once decision. Before cancellation the monitor observed `running`, `may_have_run: true`, a fixed credential-received marker, a live recipient PID and one heartbeat, with no completion marker. It cancelled that same operation; the terminal receipt was `uncertain`, error `cancelled`, `may_have_run: true`. The recipient PID was absent and its heartbeat stayed at one across two observations; no completion marker appeared. A separate later PID/marker check and CLI status query confirmed the same result. The remembered-use list stayed empty and no native prompt remained. |
| HTTP in-flight cancellation | **PASS** | Human confirmed the requested Allow once decision. Before cancellation the monitor observed `running`, `may_have_run: true`, exactly one received request with a nonempty credential header, one active stream and zero closes/completions. Cancelling that same operation yielded `uncertain`, error `cancelled`, `may_have_run: true`. Two subsequent health observations showed one request/receipt, zero active streams, one close and zero completions. An independent later status/health check confirmed the same result. No retry, remembered grant or pending native prompt appeared. |

Cancellation-plus-verification intervals were 832 ms for the process and 516 ms
for HTTP, each including an intentional 500 ms stability observation. These are
single samples, not isolated cancellation-latency measurements or performance
guarantees. The process fixture has no descendants; arbitrary process-tree
cleanup is not established by this case.

Unlike cancellation before approval, cancellation after dispatch cannot recall a
credential or undo recipient-side effects. The conservative `uncertain` receipt
is expected and must retain `may_have_run: true`, not claim that nothing ran.
Missing or uncertain status must never trigger a replay. Intentional recipient
echoes stayed behind the output boundary; the operator inspected only the fixed
markers, PID existence and closed receipts.

## Cleanup and remaining gates

Removed only the two newly registered in-flight profiles after their operations
settled. Both cancellation receipts remained queryable and unchanged after
removal. The original process/HTTP acceptance profiles were preserved. Private
reference-only fixture definitions and value-free observations were retained for
reproduction; re-registration would require fresh human approval and new IDs.

Stopped only this run's loopback browser-fixture server and streaming-HTTP server;
both owned processes exited successfully. The synthetic process had already been
reaped. No browser tab/profile, installed extension, vault, keychain item, pairing
or original acceptance server was removed. Browser website access was deliberately
restored, the extension resumed and all remembered test-use grants were revoked.
Final doctor reported the same daemon epoch ready, verified app/native-host
definitions, the retained client and one connected extension profile, with no
next steps. The consent list was empty.

This run changed no runtime/core/MagicRun/Magician source, dependencies or binary
versions. It adds native acceptance evidence, not another full automated-suite,
CI, signed-artifact or public-release pass. No secret-bearing traces or recipient
outputs were collected.

The cases listed above passed on one host. First-pairing denial, native profile/
client revocation, all-HTTPS/blocklist combinations, full-browser restart and
long-outage persistence, copied/multiple-profile native UI, late-human-answer
races, changed-recipient native refusal and maximum dialog readability still
need their separate runbook cases. Public HTTPS/provider tests, process trees,
sustained resource/performance measurements and signed/public distribution are
not qualified here. The prior intermittent automated extension-startup timeout
remains unresolved; these successful native reconnect observations do not erase it.
