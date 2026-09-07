# Security boundary and reporting

MagicVault `0.3.0` is a source alpha with scoped automated and real-CDP evidence,
not a production-qualified release. Installed-extension/native-human/keychain
and broader qualification remain open. Do not use valuable credentials until
the relevant [qualification gates](docs/testing.md) pass. Source review, a
`secure_` name, or password masking is not proof of safety.

## The supported promise

Supported agent channels use credential references, not enrolled values.
MagicVault's model-facing replies, errors, logs and audit projections do not
return those values. Trusted custody, browser adapters and native bridge code
handle material only to perform an explicitly authorized operation.

The shipped executables compile the `log` facade out (`max_level_off`): the
websocket dependency's debug/trace paths can otherwise include payloads. This
does not disable a recipient's debugger or another application's tracing. Rust
embedders must explicitly address their own dependency logging as described in
[the integration contract](docs/integrations.md).

Authorization is separate from discovery: pairing and metadata consent do not
grant browser delivery. Browser use needs per-client field/origin permission and
a native human decision for each document-bound fill. Both top-page and frame
origins are checked. No model-facing raw read, arbitrary JavaScript, generic
dispatch, material-delivery or human-grant endpoint is provided.

## What this does not protect against

- The destination website receives its credentials and can copy or transmit them.
  Filling may trigger site-defined input handlers; it is not an atomic login.
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
grant only the required sites. Use trusted, non-world-writable executable
locations. Keep the daemon root private and separate from other products.

Never put credentials in labels, field names, locators, command arguments,
examples, issue reports or screenshots. Pairing capabilities and debugging
endpoints are machine-access capabilities; do not paste them into agent chats.
No secrets, browser profiles, live configuration, telemetry payloads or private
runtime data belong in this repository.

Effects are not automatically retried after uncertainty. Cancellation cannot
recall a DOM write. Persistence uncertainty blocks new work and requires explicit
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
