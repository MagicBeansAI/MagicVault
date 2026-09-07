# Phase 2 foundation implementation ledger

Historical static-only checkpoint. The later owner-authorized
[targeted test round](targeted-tests-2026-09-06.md) records executed results and
additional SDK/fixture fixes; it does not qualify the full release.

Status: source implementation and repeated static-review checkpoint, 2026-09-06.
No checks, builds, tests, benchmarks, native operations or CI ran during this
implementation. Dependency lockfile resolution only was performed. No publication
occurred in that checkpoint; it was not a qualified release or a no-regressions certification.

The [follow-up Phase 1/2 review](static-review-2026-09-06.md) records additional
persistence, approval-replay, enrollment-deadline and MCP shutdown fixes. Current
versions are core `0.1.2`, primitives `0.1.1`, service/CLI/MCP `0.2.1`; protocol
remains `0.2.0` / wire version `1`. The findings below record the initial P2 pass.

Sequence: protocol/storage → daemon and native human boundary → pairing,
enrollment and metadata consent → CLI → SDK MCP → coordinated core consumer
integration → static review/coverage/docs. Browser delivery is the next phase;
HTTP/process follows it. No `secure_*` tool is advertised in this foundation.

The supported production interactive host is macOS. A paired client can request
access to selected credential **metadata**; a daemon-owned human decision grants
or denies that connection. This is not material delivery authority or approval
for a later effect. Enrollment collects values in native hidden-input prompts,
not in CLI/MCP arguments. Administrative requests cannot supply a grant decision.

Local authentication is OS-user ownership plus a paired client capability.
Unrestricted same-user software, accessibility control of trusted UI, debuggers,
keychain access and privileged local code are outside this support profile.
Neither filesystem modes nor hiding an MCP method isolates hostile same-user
code. The actual promise is reference/metadata-only supported agent channels.

Standalone roots/keychain identities are separate from Magician. One daemon
owns the standalone store. Shared core changes must stay additive or include
the corresponding Magician migration; standalone surfaces do not force Magician
to use a daemon or introduce a private copy of the core.

## Implemented paths

| Entry | Production path | Added, unrun coverage |
| --- | --- | --- |
| `init`, `serve`, service install/start/stop/remove | CLI → instance marker/keychain/lease → core store → Unix listener; managed LaunchAgent → same `serve` binary | Private-root/second-writer/bad-format refusal; XML escaping. Native host/keychain/service qualification still required |
| CLI pair/enroll/list/status | CLI → shared Client → authenticated framed IPC → Broker → native human + core → metadata only | Actual CLI subprocess flow, real IPC, synthetic enrollment canaries, rejected argument redaction |
| Approval and revocation | Reference-bound pending job → native human → serialized caller/expiry revalidation → metadata ACL/audit persistence | Client isolation, caller-scoped status, pending cancellation, restart, denial, revocation, stale epochs, duplicate enrollment ID |
| MCP metadata tools | Official SDK stdio binary → bounded adapter → shared Client → same IPC/Broker/core | Actual MCP subprocess plus official SDK client; catalog, metadata canaries, caller-supplied grant rejection, output bound rollback |
| Evolving core/Magician | Additive core `0.1.1` projection → existing product facade, with no standalone dependency | Core metadata projection and Magician facade type identity/reload cases; existing P1 suites retained |

## Static review findings addressed

1. Discovery originally required copying entire plaintext entries to find names.
   The additive core projection copies only IDs, labels and sorted field keys;
   it leaves old APIs and storage semantics unchanged.
2. Caller decisions and administration must not become model tools. The protocol
   rejects unknown/value/grant fields; MCP exposes exactly four implemented
   metadata/status/consent tools, with no generic dispatch or `secure_*` placeholder.
3. Consent must not hold store locks or authorize a future unspecified effect.
   Human work is serialized outside state locks; completion revalidates caller,
   reference, epoch/expiry and shutdown. The result is metadata connection only.
4. Core memory may change before persistence returns failure. The standalone
   broker poisons itself on uncertain core/ACL/audit publication, never rolls back
   optimistically, and never retries mutations after losing a reply.
5. Corruption quarantine or a missing vault/registry could look empty on restart.
   Standalone startup refuses quarantine evidence, dangling ACLs, unsafe files,
   unsupported formats and oversized state. It does not alter Magician recovery.
6. One registry size bound was too small for its declared client/reference caps.
   The 512 KiB bound accommodates the full bounded registry; a synthetic maximum
   serialization case records this invariant. IPC and MCP admission are bounded.
7. Deadline cancellation could drop a native future before child cleanup.
   Enrollment signals cancellation and awaits cleanup; shutdown drains human work
   and retains the writer lease through blocking commits. Executables return
   `ExitCode` so runtime cleanup is not bypassed by `process::exit`.
8. Parser errors could echo rejected caller input. Both binaries emit closed
   errors for parse failures; help/version remain available. Native stderr and
   SDK/OS/JSON errors never become raw protocol output.
9. A stalled MCP writer could stall shutdown; writes and close now have deadlines,
   and a failed partial frame is never retried. Serialization failure is an error
   result, never an optimistic successful tool result.
10. Pairing/profile publication cannot overwrite an existing/partial file;
    capabilities are zeroizing transport data, saved privately and never printed.
    Startup never regenerates a missing key, adopts Magician state or selects an
    insecure test backend. Tests inject fixtures through a trusted library seam.

## Remaining qualification and limits

All compilation, unit/integration/CLI/MCP tests, actual native prompts, keychain
behavior, service lifecycle, platform permissions, recovery drills, throughput,
latency, coverage percentages and Magician regression/rollback evidence remain
unrun. Source review is not evidence that they pass. The manual-only workflow was
not dispatched. Synthetic tests do not qualify Apple UI or keychain trust.

No browser, extension, HTTP, new-process or running-service material delivery is
implemented in P2. No output-sanitization guarantee covers those future surfaces.
Recovery is deliberate offline reconciliation/consistent backups, not automatic
key replacement, orphan adoption or quarantine deletion. Future delivery needs
destination/effect-bound authority and recipient/output threat modeling.

See [setup.md](setup.md) for exact entry points, limits and recovery boundaries,
and [protocol.md](protocol.md) for the supported promise and authority model.
