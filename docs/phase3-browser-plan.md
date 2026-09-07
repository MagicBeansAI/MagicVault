# Phase 3: browser credential delivery

Status: source implementation/static review complete, followed by passing
check/build/full-suite and disposable Chrome/CLI qualification, 2026-09-07.
The initial implementation checkpoint prohibited execution; later authorized
[initial verification](phase3-verification-2026-09-07.md) and
[real-world qualification](qualification/results-2026-09-07.md) are separate
records. Installed-extension/native-human/keychain acceptance, benchmarks and
wider release qualification remain outstanding. Source version `0.3.0` does
not itself announce a package or installer publication.

## Outcome and boundaries

An external agent uses `secure_fill` with credential references to fill an
already-running browser. Both the direct CDP adapter and the Chromium extension
use the same daemon-owned authorization and value-free result contract. CLI and
MCP are real clients, not mock or placeholder surfaces. Ordinary navigation and
submission remain with the caller's existing browser automation tool.

The supported promise covers MagicVault-owned agent requests, responses, errors,
logs and audit records. The receiving website necessarily receives credentials.
Other tools' DOM reads, screenshots, traces, cookies, and subsequent session
credentials are outside this delivery-only feature's filtering boundary.
Arbitrary same-user or privileged software is not isolated by this integration.

## Implementation sequence

| Milestone | Required deliverable | Review / coverage focus |
| --- | --- | --- |
| P3.1 | Closed browser protocol, target discovery and one-use effect authorization | No raw values or grant decisions in agent messages; owner/epoch/document/origin/field binding; metadata consent never authorizes delivery |
| P3.2 | Reusable `magicvault-effect` crate and direct CDP fill through daemon, CLI and MCP | Dedicated connection; no proxy; strict locators; isolated execution context; stale/ambiguous/unsupported targets; headed/headless fixtures |
| P3.3 | Credential-focused Chromium extension, native host and authenticated daemon bridge | Allowed extension identity, explicit site permissions, isolated execution, document targeting, bounded transport, reconnect/cancellation/uncertain outcome |
| P3.4 | Builder contract, installation/removal, examples and public README | Fresh user journey for both backends; no private machine paths or credentials; current capability versus roadmap table |
| P3.5 | Repeated whole-path static review and focused coverage | Approval/expiry/revocation races, partial effects, no replay after uncertainty, cleanup, bounded memory/concurrency, own-channel secret canaries, foundation regression |

CLI/MCP wiring accompanies the first CDP milestone, not a later integration
exercise. Completion requires both backends, setup, builder documentation and
coverage sources. Unexecuted qualification is recorded explicitly.

P3.1–P3.5 are implemented and statically reviewed at this checkpoint. The
[implementation/review ledger](phase3-review.md) maps the deliverables to source
and records fixes and actual tool activity. Automated and scoped real-CDP/CLI
verification now passes; it does not qualify the installed extension or certify
a production release. Remaining [qualification gates](testing.md) are explicit.

## Compatibility and long-term direction

- Keep `magicvault-core` and `magicvault-primitives` APIs, data formats, keys,
  policy semantics and dependency graphs unchanged whenever possible.
- Standalone browser policy is explicit and deny-by-default. Credentials enrolled
  under the metadata-only foundation must not become deliverable automatically.
- Magician continues embedding shared custody; it must not acquire a daemon,
  extension or CDP dependency through the core. No live consumer store is opened.
- MagicRun is not required for browser delivery. HTTP and new-process execution
  follow in Phase 4; general running-service adoption is later.
- Initial interactive host: macOS. Qualification targets headed and modern
  headless CDP Chrome/Chromium, and a headed extension installation. Other
  browsers/platforms and headless-extension combinations require separate evidence.
- No generic browser JavaScript, navigation, screenshot, cookie-read, material-read
  or human-approval-grant operation is added to the agent surface.

## Review ledger

1. Planning review: existing metadata consent cannot authorize browser delivery.
   Browser setup and each fill need their own actual-target-bound authority.
2. Planning review: the inherited store policy permits only metadata. Add explicit
   standalone browser authorization instead of silently widening existing entries
   or changing shared-core policy evaluation for embedded consumers.
3. Planning review: external snapshot references and connection-local CDP node IDs
   are not interchangeable. Use strict CSS selectors in an explicitly bound
   document; never accept arbitrary caller JavaScript or guess reference mappings.

Implementation findings, changes and unexecuted evidence are recorded in the
[review ledger](phase3-review.md). The README does not describe this checkpoint
as a production-qualified release.
