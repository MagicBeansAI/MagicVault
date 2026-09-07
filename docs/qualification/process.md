# Process and HTTP delivery qualification

The `0.4.0` standalone surface implements fixed-profile new-process and HTTP
delivery. Automated conformance uses actual local recipients, the public MagicRun
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
| Authority | Closed UUID-only requests; unknown overrides rejected; native registration/per-use seams; cross-client and wrong-field refusal |
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

## Native acceptance (manual, not replaced by synthetic approval)

Use a disposable OS account and private fresh root. Initialize, serve, pair and
enroll a public synthetic token via real native hidden input. Never reuse an
embedded application's root or credentials. Follow [setup](../setup.md).

1. Build the standalone binaries. Prepare a trusted fixed recipient that accepts
   a synthetic env/stdin value and writes a non-secret completion marker in a
   disposable directory. Review its program, inputs and permissions.
2. Register its exact process profile through the real CLI and review the entire
   native dialog. Deny once and verify no profile/effect; approve deliberately.
   Confirm listing returns only label/kind/profile ID.
3. Invoke once, deny per-use consent and verify no marker. Invoke a fresh ID for
   a deliberately approved new operation; verify the marker and closed receipt.
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
