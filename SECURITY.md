# Security boundary and reporting

MagicVault `0.8.1` is a source alpha, not a production-qualified release.
Basic installed-browser/native-human/keychain and selected remembered-use,
permission/recovery and in-flight cancellation cases passed on one host. Broader
native acceptance and startup/process-delivery reliability remain open. Do not
use valuable credentials until the relevant [qualification gates](docs/testing.md) pass. Source review, a
`secure_` name, or password masking is not proof of safety.

## The supported promise

Supported agent channels use credential references, not enrolled values.
MagicVault's model-facing replies, errors, logs and audit projections do not
return those values. Trusted custody, browser/HTTP/process adapters and native bridge code
handle material only to perform an explicitly authorized operation.

The shipped executables compile the `log` facade out (`max_level_off`): the
websocket dependency's debug/trace paths can otherwise include payloads. This
does not disable a recipient's debugger or another application's tracing. Rust
embedders must explicitly address their own dependency logging as described in
[the integration contract](docs/integrations.md).

Authorization is separate from discovery: pairing and metadata consent do not
grant browser delivery. Browser use needs per-client field/origin permission and
a native per-use decision or an explicitly human-created exact-use grant. Both
top-page and frame origins are checked on each operation. New-process and HTTP
delivery requires human-registered fixed recipient profiles plus per-use or
remembered consent for that exact profile and paired client. Profile literals/labels
are public configuration, not places for credentials. No model-facing raw read, arbitrary JavaScript, generic
dispatch, material-delivery or human-grant endpoint is provided.

## What this does not protect against

- The destination website receives its credentials and can copy or transmit them.
  Filling may trigger site-defined input handlers; it is not an atomic login.
- An authorized child process or HTTP service receives credentials too. Process
  digest binding is not a sandbox or a recursive attestation of interpreters,
  arguments' files, libraries or recipient network activity. Trust the whole
  recipient and its relevant inputs. Plain loopback HTTP is unencrypted and only
  suitable for trusted local recipients; use synthetic data during qualification.
- Process/HTTP results deliberately withhold stdout/stderr, exit codes, response
  bodies/headers and raw HTTP status codes, including transformed echoes. Timing
  and coarse success/failure remain observable; this is not a noninterference
  guarantee. Separate tools, recipient logs and created files are not filtered.
- A separate browser tool may read DOM values, screenshots, cookies, traces or
  newly created session credentials. Filtering those paths is deferred.
- Privileged or unrestricted same-user code may read pairing files, drive native
  UI, invoke a trusted host directly, debug processes, access an authorized
  keychain or control a debugging endpoint. This is not a process sandbox.
- Native manifest identity and origin checks constrain normal Chrome extension
  connections; they do not cryptographically attest a caller against unrestricted
  same-user software impersonating Chrome.
- Trusted library consumers receive plaintext. A third-party integration must
  enforce its own caller/target policy and model-output boundary; linking a crate
  alone does not confer the product guarantee.
- Zeroization shortens some in-process lifetimes. Rust transport allocations,
  browser/JavaScript heaps and recipient DOM storage are not claimed to be
  completely erased. No “nobody can ever read your credentials” claim is made.

## Operational safeguards

Use a dedicated automation profile and explicit loopback CDP endpoint, never a
network-exposed debugger. Install only trusted extension/native host code, and
prefer selected-site grants. The extension also offers explicitly approved
all-HTTPS access; local HTTP remains opt-in. Broad access increases the impact of
compromised extension code and discovery's reach, not credential authorization.
The local site blocklist suppresses MagicVault discovery/fills for both top and
frame sites; it does not revoke Chrome permissions or protect against a compromised
extension. Trusted local storage contains site settings and a random browser-profile
pairing capability, never enrolled credentials or fill payloads. The daemon stores
only the capability hash and remembered authorization/refusal. This authorizes
reconnection/discovery, not credential use. Separately, the daemon can remember an
explicit Always allow decision for an exact browser/field scope; [consent scope
and revocation](docs/consent.md) describe its wider lifetime and limits.
Pause stops local retries; daemon-side browser/client revocation removes authority.
Neither recalls dispatched fills. Reinstalling clears local settings but does not
revoke the old daemon-side grant. The public manifest key fixes unpacked identity,
not publisher authenticity or resistance to unrestricted same-user impersonation. Use trusted, non-world-writable executable
locations. Keep the daemon root private and separate from other products.

Never put credentials in labels, field names, locators, command arguments,
examples, issue reports or screenshots. Pairing capabilities and debugging
endpoints are machine-access capabilities; do not paste them into agent chats.
No secrets, browser profiles, live configuration, telemetry payloads or private
runtime data belong in this repository.

Effects are not automatically retried after uncertainty. Cancellation cannot
recall a DOM write, child execution or HTTP side effect. Persistence uncertainty blocks new work and requires explicit
reconciliation; missing keys and damaged state are not silently regenerated or
adopted. Native removal only touches validated managed definitions, never vaults.

## Report a vulnerability

Use GitHub's private vulnerability reporting facility for this repository when
available. If it is unavailable, request a private reporting channel from the
maintainers without publishing exploit details or sensitive artifacts. Do not
file public issues containing credentials, pairing files, vaults or browser
profiles. A minimal reproduction should use synthetic data and a disposable
profile, and identify the affected version and integration surface.

There is no declared production-supported release or response-time SLA for this
alpha. Security fixes and their limits are recorded in the changelog and the
linked technical documentation without disclosing user credentials or private operations.
