# Process and HTTP delivery qualification

The standalone surface implements fixed-profile new-process and HTTP delivery,
introduced in `0.4.0`. Version `0.8.0` adds native exact-use remembered consent.
Automated conformance uses actual local recipients, the public MagicRun
coordinator, real IPC and shipped CLI/MCP executables, with synthetic custody and
human providers. It never approves a live credential or contacts a real provider.
[Usage and trust boundary](../delivery-usage.md).

## Run deterministic product tests

```bash
make print-target-dir
make test-delivery
```

This is a focused lane. `make test` includes these tests in the full workspace
suite. All Cargo lanes use the selected SSD1 build directory when available.
Run MagicRun's full suite from its own checkout/build tree when qualifying the
execution dependency. Record actual results using the
[results template](result-template.md); do not claim tests ran merely because
the source exists.

| Concern | Executable coverage |
| --- | --- |
| Authority | Closed UUID-only requests; unknown overrides rejected; native registration/use seams; cross-client and wrong-field refusal |
| Remembered use | Default asks again; exact immutable profile scope; restart persistence; per-client revocation/reset; late answers and failed writes cannot bypass revocation |
| Process delivery | Real child receives exact synthetic env/stdin through MagicRun; changed executable and unsafe loader variables refused |
| Process output/lifecycle | Echoed stdout/stderr withheld, cancellation and output flood bounded; closed dispatch evidence, no implicit second launch |
| HTTP methods/placement | GET/HEAD/POST/PUT/PATCH/DELETE/OPTIONS/TRACE/PROPFIND; authorization headers, encoded query, text/form/flat JSON |
| TLS and destinations | Real local TLS exchange with test-only trust root; untrusted/wrong-host certificates rejected; special-use addresses and nonpublic DNS refused |
| HTTP response/uncertainty | Raw and encoded echoes discarded; redirects/lost replies cause no repeat; output cap, stalled body, cancellation and invalid headers |
| Broker lifecycle | Deny/cancel/remove/shutdown, profile capacity, detailed-result eviction with spent IDs, persisted profiles and old-epoch refusal |
| Durable reconciliation | Final audit-write failure blocks new effects and remains queryable through actual CLI IPC |
| Actual client wiring | CLI and official-SDK MCP subprocesses invoke both production adapters; existing browser flow remains covered |

The TLS fixture's custom root and loopback resolution are test-only transport
seams. They do not establish access to a public provider or expose an insecure
mode in the product. The OS-native trust store, desktop prompt rendering and
keychain path still require platform acceptance.

## Focused intermittent-process investigation

Dispatch **Focused process investigation** on a reviewed revision for the
original macOS runner environment. This manual workflow builds uninstrumented
unsigned clients, validates the closed diagnostic observer/release guard, then
starts at most 200 independent diagnostic test executables. Each creates new
synthetic custody, recipients and operation IDs. The original process test and
its HTTP companion still run concurrently; there is no automatic delivery replay.

For a deliberately selected local package candidate:

```bash
node scripts/qualify-package.mjs --packages "$candidate_dir/packages" \
  --work "$candidate_dir/process-investigation" --app-parent /private/tmp \
  --with-rust-tests --with-process-diagnostics --process-investigation-rounds 200
```

The mode accepts 1–200 trials and refuses combination with browser/performance
or repeated full-package qualification. After compilation/preflight, a monotonic
ten-minute trial budget caps each driver to the lesser of two minutes and the
remaining budget. Timeout kills the exact test driver, not arbitrary process
groups. This mode is restricted to the fixed short-lived synthetic recipients;
it is not a supervisor for arbitrary programs or a production cancellation API.
The CI job has a separate 25-minute ceiling, including compilation and teardown.
Failure or budget exhaustion produces a nonzero exit and stops further trials.
Passing all requested trials reports `inconclusive_no_reproduction`: keep earlier
failures open. Do not retry a failing run for green or infer the signal's sender
from the signal class alone. [Evidence](results-focused-candidate-2026-09-08.md).

## Native acceptance (manual, not replaced by synthetic approval)

Use a disposable OS account and private fresh root, or an explicitly authorized
dedicated installation in the current account. Initialize, serve, pair and
enroll a public synthetic token via real native hidden input. Never reuse an
embedded application's root or credentials. Follow [setup](../setup.md).

1. Build the standalone binaries. Prepare a trusted fixed recipient that accepts
   a synthetic env/stdin value and writes a non-secret completion marker in a
   disposable directory. Review its program, inputs and permissions.
2. Register its exact process profile through the real CLI and review the entire
   native dialog. Deny once and verify no profile/effect; approve deliberately.
   Confirm listing returns only label/kind/profile ID.
3. Run the consent matrix below separately for process and HTTP. Use a fresh ID
   for each deliberate operation; reconcile the same ID if transport is uncertain.
   Recipient echo must never appear in CLI/MCP output or typed audit.
4. Change executable bytes and verify the old profile refuses dispatch. Remove
   and deliberately re-register after reviewing the change.
5. Use a trusted loopback HTTP fixture with a synthetic credential, then a
   controlled public HTTPS test endpoint if separately authorized. Verify the
   selected placements at the recipient and no raw response in model-facing
   output. Never authenticate to a live account or send a real provider token.
6. Exercise cancellation while awaiting consent and after dispatch, terminal
   status, profile removal, client revocation, daemon restart and uncertain
   transport. Missing status must not trigger automatic retries.
7. Review maximum-size profile dialogs for legibility and untruncated destination/
   placement details. A byte-length assertion does not qualify the native UI.
8. Record native OS/keychain behavior, executable/build revisions, receipt-only
   expectations and failures. Keep private profiles, keychain identities and
   capabilities out of public artifacts.

Measure latency/CPU/memory, repeated operation stability, concurrent discovery,
deadline and shutdown behavior separately. Fast test duration is not a benchmark.
No passing integration suite proves arbitrary recipient software trustworthy.

### Native consent-mode matrix

Keep a value-free recipient counter. A process can append one fixed completion
marker; HTTP can expose a separate counter-only health endpoint. Do not inspect
the delivered credential or an echo response directly. Registration and use are
separate decisions, and a timeout/refusal alone does not prove a human clicked Deny.

| Case | Human action | Required observation |
| --- | --- | --- |
| D-01 registration deny | Deny the proposed exact profile | No registered profile and no recipient execution |
| D-02 registration allow | Review and approve a new registration | Reference-only profile ID; registration itself performs no delivery |
| D-03 use deny | Deny a fresh operation | Denied receipt, unchanged recipient counter, no remembered grant |
| D-04 allow once | Select Allow once, then deliberately request a fresh use and deny it | One delivery only; second use asks again; no remembered grant |
| D-05 always allow | Select Always allow, then deliberately request a fresh matching use | Both deliver once each; second use needs no native use prompt; one exact-scope grant |
| D-06 revoke | Revoke that grant, request a fresh use and deny | New prompt, no new delivery; other scopes/clients unchanged |
| D-07 restart | Explicitly remember a scope, reconcile settled work, restart only the owned daemon | Grant survives, new operation works; old/missing operation is never replayed |
| D-08 cancel/reset race | While native consent is pending, cancel or clear consent; answer only if the dialog remains | No dispatch or restored authority from a late answer |
| D-09 changed recipient | Change executable bytes or register a new profile | Old executable constraint still refuses; new profile cannot inherit the old grant |

Record each native button choice and its closed receipt independently. Synthetic
provider tests cover the logic but do not qualify actual dialog rendering or
keychain interaction. [Scope, persistence and recovery semantics](../consent.md).

## Reusable fixtures and running services

```bash
make test-qualification-fixtures
```

`test-support/process-fixtures/child.mjs` supplies env, stdin, echo and stateful
JSON-lines probes. Those fixture-only tests validate recipient behavior, **not**
the MagicVault product path above. Echo mode is deliberately unsafe as model
output and accepts only synthetic data.

An already-running process needs an explicit cooperating credential-provider,
IPC, reload or refresh contract. The stateful fixture demonstrates such a seam;
MagicVault does not yet implement live rotation, update another process's
environment, take over arbitrary PIDs/PTYs or refresh an existing connection pool.

Shared-core and MagicRun consumer contracts are retained. Magician-owned runtime
regression tests remain the consumer's responsibility; unchanged source plus
standalone tests is not a claim that Magician's full suite ran.
