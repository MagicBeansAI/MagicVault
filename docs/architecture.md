# MagicVault architecture

Architecture version: `0.6.0`

Previous immutable baseline tag: `architecture/v0.3.0`. The current reviewed
source/document baseline is [architecture-baseline.json](architecture-baseline.json).
Updating this document does not create a Git tag or announce a package release.

This is the technical baseline for standalone browser, process and HTTP delivery and
its shared libraries. The version labels the architecture, not a production
certification or a registry publication. [Capabilities](../README.md#what-works-today)
and [security](../SECURITY.md) define the product's supported boundary.

## Components and trust boundaries

```mermaid
flowchart TB
    agent["Agent / model"] -->|"References and locators only"| client["CLI or MCP client"]
    client -->|"Authenticated local IPC"| broker["Standalone service broker"]
    human["Human via native UI"] -->|"Pairing, policy and per-use consent"| broker
    broker -->|"Resolve approved fields"| core["Shared custody core"]
    keychain["OS keychain / host key provider"] --> core
    core --> storage["Encrypted vault and typed audit"]
    core --> primitives["Durable I/O and bounded JSON primitives"]
    broker -->|"Authorized material"| effect["Trusted browser effect adapter"]
    broker -->|"Fixed profile and authorized material"| http["HTTP adapter: vetted/pinned destination, verified TLS"]
    broker -->|"Fixed profile, executable digest and material"| process["Process adapter"]
    process --> run["MagicRun public governed batch coordinator"]
    run --> child["New authorized child: env / stdin"]
    http --> service["Authorized HTTP recipient"]
    child -->|"Raw output withheld"| run
    service -->|"Response content withheld"| http
    run -->|"Closed dispatch outcome"| process
    process -->|"Value-free receipt"| broker
    http -->|"Value-free receipt"| broker
    effect -->|"Dedicated loopback CDP connection"| browser["Existing browser document"]
    effect -->|"Authenticated native bridge"| host["Native messaging host"]
    host --> extension["Chromium extension: browser grants + local site blocks"]
    extension -->|"Fixed isolated fill function"| browser
    browser -->|"Closed delivery outcome"| effect
    effect -->|"Status, never values"| broker
    broker -->|"Value-free results"| client
    consumer["Embedded application such as Magician"] --> core
    other["Existing browser automation tool"] -->|"Navigation and observations outside this filter"| browser
```

The client, broker, custody library and delivery adapter are different authority
boundaries. CLI/MCP requests contain references, not credential values. Trusted
core/effect/native code necessarily handles plaintext. The destination website
receives it; separate browser tools and unrestricted same-user software are not
isolated by this integration. New processes and HTTP services receive plaintext
inside their authorized boundary too. Neither CDP nor the extension is a generic
proxy. Process/HTTP adapters withhold all recipient output instead of attempting
generic substring redaction; coarse outcomes and timing remain observable.

| Component | Version | Ownership |
| --- | --- | --- |
| `magicvault`, `magicvault-mcp` | `0.6.0` | Human administration and reference-only clients; CLI also builds daemon/native host and owns explicit setup |
| `magicvault-service` | `0.6.0` | One standalone store writer and separate private application-bundle installer |
| `magicvault-protocol` | `0.4.0`, unchanged | Caller authentication, policy, consent, destination profiles, jobs and bounded IPC |
| `magicvault-effect` | `0.4.1` | Dedicated CDP/native fills, HTTP transport and trusted MagicRun process integration |
| Chromium extension | `0.5.0` | Explicit all-HTTPS or selected-site grants, local blocks and native document-targeted fill; no navigation or submission API |
| MagicRun `tool-runtime-core` | `0.1.73`, existing public Git dependency | Governed process preparation, digest-bound dispatch, cancellation, output bounds and owned-child cleanup; runtime source unchanged |
| `magicvault-core` | `0.1.3` | Encryption, credential references, existing policies, scoped stores and typed audit |
| `magicvault-primitives` | `0.1.1` | Durable filesystem and stack-safe JSON utilities |

The local agent wire is version **3**, unchanged. The profile-authenticated native
handshake is version **2**; effect commands and host configuration remain version **1**.
Wire versions and crate versions are distinct. See the [protocol](protocol.md)
for framing, message types and bounds, and [versioning](versioning.md) for upgrades.

## Distribution and stable application lifecycle

```mermaid
flowchart LR
    release["Reviewed build / optional explicit Apple signing"] --> bundle["Allowlisted platform package + SHA-256 manifest"]
    npm["Exact-version npm launcher"] --> bundle
    npm -->|"stdio / arguments unchanged; no custody"| cli["Native CLI / MCP"]
    bundle -->|"Explicit setup; bounded streaming copy"| app["Private immutable versions + atomic current symlink"]
    app --> daemon["Stable LaunchAgent executable"]
    app --> mcp["Stable MCP configuration"]
    app --> host["Stable native host / unpacked extension"]
```

Distribution does not move credential custody into JavaScript. npm has no lifecycle
hooks, runtime downloads, daemon initialization or native enrollment. The launcher
checks the selected platform/version and binary hash before an argv-preserving,
shell-free spawn; stdout belongs exclusively to the native protocol.

`~/.magicvault-app` is separate from `~/.magicvault`, with a root-bound ownership
marker and installer lock. Fixed-path, bounded, no-follow reads stream into private
version directories; a durable bundle marker follows all file/directory syncs.
Activation atomically publishes only a relative `current` symlink after complete
reverification. Existing versions and ambiguous staging artifacts are retained.
Same-user unrestricted software and a compromised publisher remain outside this
isolation boundary; integrity manifests do not replace publisher authentication.

Native setup explicitly initializes/starts/pairs with existing human consent.
Upgrade/uninstall require exact managed service ownership and wait for the custody
writer lease after unloading, before activation or recoverable app retirement.
Stable paths outlive npm/npx caches; clients obtain fresh handles after upgrades.
Normal setup additionally registers the bundled extension's exact public identity;
install-only does not. The native installer serializes updates with a private
root lock and repairs only exact managed definitions, refusing foreign/modified
files or a different root/client/executable. Config is published last, so an
interrupted identity migration may deny connections but cannot broaden access.
Upgrade preserves explicitly selected custom IDs; setup selects the bundled ID.
Normal setup and `doctor` project an advisory extension-readiness snapshot from
bounded private native-definition reads and the existing caller-scoped
`ListBrowsers` RPC (two-second probe deadline). Registration verification checks
exact manifests/wrapper, executable safety and local pairing structure without
repairing files; it is not browser-extension installation detection or publisher
verification. Only a live extension connection confirms presence. No connection
leaves installation unconfirmed; CDP connections never count. Diagnostics neither
scan browser profiles nor grant authority, and do not gate other delivery surfaces.
Uninstall preserves the vault/keychain/pairings. No new agent tool or agent wire
change is introduced; the native handshake requires a coordinated upgrade. Core, primitives, MagicRun and Magician need no source or data
migration. [Installation/recovery](distribution.md) covers partial-state behavior.

The architecture gate includes npm launchers, package assembly/qualification,
signing script and distribution workflow as executable trust inputs. Release
credentials/publication remain separate authorized operator actions; the checked-in
workflow produces explicitly unsigned local-tarball candidates only.

## Browser-profile connection lifecycle

```mermaid
flowchart LR
    profile["Extension profile: random ID + local capability"] --> host["Exact-origin native host"]
    host --> auth["Daemon: client + extension + profile + capability hash"]
    auth -->|"First use / explicit reapproval"| human["Native human decision"]
    human --> saved["Durable allow / refusal"]
    saved -->|"Allowed; no duplicate live identity"| live["Independent connection + fresh browser handle"]
    live -->|"Transport unavailable / busy"| retry["Durable backoff + Chrome alarm"]
    retry --> auth
    auth -->|"Denial / revocation / duplicate / version error"| pause["Stop; human Retry required"]
```

Only the trusted worker stores the random 256-bit reconnect capability, alongside
site settings, pause state and retry deadline; it never syncs or returns the
capability through options/agent messages. The daemon persists up to 128 hash-bound
profile decisions scoped to client/extension/profile, including a refusal before
first consent. Reconnects reuse approval without taking the human-prompt gate.
Client revocation removes grants; browser disconnect revokes just its native
profile. Local Pause does not erase an existing approval. Duplicate/copied profile
identities never evict a live connection. Nonblocking socket EOF detection reaps
quiet dead hosts and cancels old pending effects without consuming replies. A
dedicated cloned socket descriptor keeps health probes off the effect mutex, so
concurrent status polling cannot make delivery spuriously busy. Disconnect shuts
down both descriptors' shared socket, rather than retaining a silent open peer.

Chrome alarms implement 30–300-second exponential backoff plus up to five seconds
of jitter for transport/busy failures, not exact-time scheduling. Worker startup
restores lost alarms and respects the persisted deadline/pause. Terminal errors
require explicit Retry; every effect remains one-use, never replayed by reconnect.
Native framing validates closed greetings and commands; v1 handshake downgrade is
refused. Browser permissions, per-credential rules and per-fill consent are unchanged.
The fixed public unpacked identity is not a signing key or a Store listing.

## One fill, end to end

Extension access is separate from credential authorization. Chrome grants remain
optional: a human chooses all HTTPS once or selected hosts; local HTTP is always
separate. Existing grants are retained on upgrade. Resetting to selected sites
clears HTTPS grants, not local HTTP or blocks. A bounded, versioned site blocklist
in trusted-context-only `chrome.storage.local` stores no material and does not sync.
Only the exact extension setup page can ask the worker to mutate it; one writer
prevents lost updates between setup tabs. Storage failures/corruption fail closed.
Discovery reads policy once per enumeration; document-bound fills re-read policy
after asynchronous document checks and recheck Chrome permissions before dispatch.
Policy-write and permission-change revisions invalidate in-flight preflight;
changes after dispatch cannot recall an effect. Blocks cover exact scheme/host
across ports, apply to top and selected frames, and are not Chrome permission
revocation or protection from compromised extension code. No crate or wire change
is needed; the fixed fill function and Magician's shared custody remain unchanged.
The setup page reads its persistent access indicator directly from Chrome,
independently of worker/blocklist availability. Permission events and page return
refresh it without polling or caching a user click as authority; stale reads are
discarded and permission-read failures show an unverified state.

1. A human pairs a client and enrolls values through native hidden inputs. Listing
   metadata does not grant delivery. The human separately authorizes selected
   fields and exact origins for that client.
2. CDP registration or an approved extension connection creates a caller-owned
   browser handle. Discovery binds short-lived targets to the tab/frame, actual
   document and top/selected origins; another tool's snapshot IDs are not handles.
3. `secure_fill` admits a closed reference-only request, spends its target handle
   and operation ID, and requests consent for that exact destination and fields.
4. Immediately before delivery, the service rechecks authority, cancellation,
   expiry and target binding, then resolves material through the custody core.
   Browser I/O and human waits do not hold the custody lock.
5. The adapter executes a fixed isolated fill function. Strict input preflight
   precedes the first write, and later controls are revalidated before each write.
   The original automation tool retains browser ownership and form submission.
6. The service persists a typed completion receipt and returns only status.
   Partial writes, lost replies and audit uncertainty are not successful rollback
   and never cause automatic retries. Faulted custody permits status reconciliation
   while refusing new effects.

Detailed results expire; spent IDs remain refused for the daemon epoch. Restart
invalidates connections/handles/jobs. Missing status is not proof that a fill
did not happen. [Lifecycle and recovery](browser-usage.md#request-a-fill-and-inspect-its-outcome).

## One new process or HTTP request

1. A human registers a caller-owned, immutable profile containing reference-only
   credential placements and an exact executable/argv/cwd or URL/method. Process
   registration captures an executable content digest; no effect runs here.
2. The agent selects that profile and an operation UUID. Admission reserves the
   one-use identity and shared human/effect permit before prompting. Registration
   is not per-use consent, and metadata access is not effect authority.
3. After consent, the broker rechecks caller/profile/field authority, cancellation
   and deadline under its writer, appends durable authorization, and resolves the
   selected material once. The `running` transition is the dispatch-authority
   boundary; revocation afterward requests cancellation, not retroactive rollback.
4. No child/network I/O holds the writer. Process work goes through MagicRun's
   existing public coordinator with a clean environment, exact arguments and the
   registered executable digest. HTTP uses vetted/pinned public destinations,
   verified TLS, no redirects/ambient proxy/retries, and bounded response discard.
   Explicit loopback IP HTTP is allowed for trusted local recipients only.
5. Child stdout/stderr/exit codes and HTTP bodies/headers/raw status codes never
   become agent output or audit data. The broker durably settles only typed IDs,
   kind, coarse state, `may_have_run` and a closed error. Audit failure faults
   custody and preserves authenticated status reconciliation.
6. The workflow keeps the shared permit through owned-child cleanup and final
   audit. Profiles persist; jobs expire, with epoch-local spent-ID tombstones.
   Missing results and new epochs never authorize automatic replay.

Profiles are capped at 16 × 12 KiB to fit the existing 1 MiB registry alongside
maximum ACL/browser state. There are 32 retained delivery jobs, 4096 spent IDs,
1–120 second effect deadlines, 64 KiB per child output stream and 1 MiB discarded
HTTP body. System DNS waits are bounded; up to two credential-free OS resolver
threads retain their permits even if a caller times out. They cannot accumulate
or hold Tokio shutdown open. [Exact formats and limits](delivery-usage.md).

The process adapter is not a sandbox, whole-program attestation or arbitrary
running-process injector. Script inputs, libraries and recipient-selected
network activity remain trusted. New HTTP does not update a stateful client or
existing connection pool. The shared core, primitives, MagicRun runtime and
Magician need no API, source or storage migration for these standalone additions.

## Integration invariants

- **Core isolation:** shared core/primitives do not acquire the standalone
  daemon, browser adapter, MCP or extension as dependencies. Embedded consumers
  keep their own roots, key identities, policy and execution ownership.
- **Authorization before material:** client/field/origin/document policy and
  native per-use consent precede custody resolution and delivery. No raw-value
  model tool or production auto-approval switch is part of the contract.
- **Transport separation:** agent IPC is reference-only; trusted native/CDP
  transport can carry values. Shipped executables compile dependency payload
  logging out; Rust embedders must enforce their own logging boundary.
- **Browser identity:** direct CDP requires a supported loopback browser websocket
  and a system-unique execution context. The extension needs exact native-host
  identity, site permission and explicit document targeting. Opaque or unsupported
  frames/controls fail closed.
- **Bounded, non-replaying work:** admission, messages, contexts, jobs, deadlines
  and audit output are bounded. Disconnecting this integration never closes the
  user's browser. Cancellation cannot recall delivered material.
- **Honest scope:** new HTTP/process profiles are standalone surfaces, not a
  general-purpose programmable secret client. Existing-service refresh, PTY/PID
  injection and other tools' observation filtering are not exposed. Recipients
  can copy credentials. MagicRun is a dependency of the effect crate, never of
  shared custody, and is invoked only for new-process delivery.

## Source map

| Responsibility | Source |
| --- | --- |
| Closed browser contract | `magicvault-protocol/src/browser.rs` |
| Authority, consent and operation state | `magicvault-service/src/broker/browser.rs` |
| Closed process/HTTP profiles and receipts | `magicvault-protocol/src/delivery.rs` |
| Profile authority and delivery lifecycle | `magicvault-service/src/broker/delivery.rs` |
| Trusted material, HTTP and MagicRun process adapters | `magicvault-effect/src/delivery.rs`, `http.rs`, `process.rs` |
| Store ownership and authenticated transport | `magicvault-service/src/storage.rs`, `ipc.rs`, `client.rs` |
| CDP and native adapter | `magicvault-effect/src/cdp.rs`, `bridge.rs`, `fill.js` |
| Native installation and executable | `magicvault-service/src/native.rs`, `magicvault/src/bin/magicvault-native-host.rs` |
| Browser extension | `extension/worker.js`, `options.js`, `manifest.json` |
| Shared custody and utilities | `magicvault-core/src`, `magicvault-primitives/src` |

## Detecting architectural drift

[architecture-baseline.json](architecture-baseline.json) binds this document to
the current package versions and SHA-256 fingerprints of workspace/package
manifests, production `src/` files, the root lockfile and extension assets.
New/removed inputs, dependency changes, source edits or a version/document mismatch
make `make check-architecture` fail. `make check` and manual qualification CI
include that gate. No Git hook, push or publication is implied.

The fingerprint is deliberately coarse: it flags possible drift, including
non-architectural source edits, but cannot prove semantic correctness or prevent
an operator from approving a bad baseline. It is not a security attestation or a
replacement for [conformance tests and native acceptance](testing.md).

When it fails, review the changed boundaries and update this document's diagram,
invariants and version when needed. Then inspect the candidate from
`make -s architecture-snapshot`, replace the baseline **only after that review**,
run `make check-architecture test-architecture`, and commit code, document and
baseline together. Never refresh the fingerprints simply to silence the gate.
The guard's member-discovery rules must also be reviewed if the workspace layout
changes. Documentation-only changes outside this architecture file do not
invalidate production fingerprints.
