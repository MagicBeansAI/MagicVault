# Building on MagicVault

MagicVault separates custody, trusted delivery and agent-facing operations. You
can embed Rust libraries, call the standalone daemon, or integrate an existing
Chromium extension. No Magician runtime is required. Current browser code is an
unqualified alpha; [qualification](testing.md) applies to third-party adapters too.

## Rust application or browser-tool builders

Use `magicvault-core` for custody. Supply `MasterKeyProvider` and, when using
scoped resolution, `SecretScopeLayout`. Keep your application's identity, root,
approval and runtime ownership explicit. The crate is a trusted in-process API,
not a sandbox or a raw-material tool for your model. `magicvault-primitives` can
be consumed independently for durable filesystem and stack-safe JSON operations.

For browser delivery, `magicvault-effect::BrowserAdapter` supplies:

| Method | Trusted input/result | Host responsibility |
| --- | --- | --- |
| `targets(cancel)` | Backend tab/frame/document bindings and safe origins | Authenticate caller; project only permitted metadata into agent handles |
| `fill(target, fields, cancel)` | Bound target; `MaterialField` CSS/value pairs; closed `Outcome` | Check exact destination/field policy and human consent before constructing material |
| `disconnect()` | Ends this integration, never the browser | Invalidate handles, cancel pending jobs, reconcile uncertain effects |
| `connected()` | Local adapter health | Do not treat this as proof a remote target still exists |

The shipped CDP implementation is `CdpBrowser::connect`. It accepts only an
explicit loopback browser websocket, uses its own bounded connection and fixed
isolated-world function, and exposes no arbitrary CDP dispatch or caller JS.
`MaterialField` intentionally has no `Debug` or `Clone` implementation and
zeroizes its owned value on drop. This does not guarantee complete heap erasure
inside a browser or third-party transport library.

Do not hand these material-bearing types to an agent serializer. A host must
implement the same one-use/document/caller binding, no-retry and value-free
result boundary as the standalone service, or clearly state a narrower contract.
The adapter itself does not enforce your custody policy or obtain human consent.
Keep shared custody independent of effect/backend dependencies.

The websocket dependency can log raw messages at debug/trace levels. The shipped
executables enable `log`'s `max_level_off` and `release_max_level_off` features,
disabling that facade in both debug and release builds. A reusable effect/core crate does not globally change
its host application's logging configuration. If embedding CDP directly, enable
the same features in your final application, or permanently suppress dependency
payload logging before any connection and throughout its lifetime. A temporary
log-level change around a fill is not concurrency-safe. Never send transport
traces or raw exceptions to a model. Cargo logging features unify across a
dependency graph; intentionally review that tradeoff when choosing the
compile-time safeguard (including when embedding the MCP crate).
Conflicting `max_level_*`/`release_max_level_*` features must be resolved, not
worked around by re-enabling payload logging.

## Application and SDK builders in other languages

Prefer the reference-only local protocol on `rpc.sock`, documented in
[protocol.md](protocol.md). CLI and MCP are optional: a local Python/Node/other
client can pair through the human setup flow, authenticate with its privately
stored capability, inspect status/epoch, discover handles, and request fills.
The capability is machine authentication data, not a value to put in chat.

Implement bounded length-prefixed frames and closed schemas. Use protocol version
2, verify local peer identity, and never retry a mutation because a reply was lost.
Use a fresh operation UUID once for `secure_fill` and retain it for status lookup.
After a daemon restart, discard old handles; missing status is not safe retry.
Do not make an administrative command into an implicit human-approval bypass.

The shipped CLI is a convenient reference-only client for languages without a
dedicated SDK. There are no promised Python/Node packages or automatic bindings
in Phase 3. Setup examples are in [browser usage](browser-usage.md).

## Existing extension builders

You can implement the same native bridge in an existing trusted Chromium
extension, instead of asking users to install a second extension. The shipped
`extension/worker.js` is a reference implementation; packaging adds the fixed
fill function from `magicvault-effect/src/fill.js`.

1. Obtain explicit site permissions and connect to native host
   `ai.magicbeans.magicvault` from your service worker/extension context.
2. Install a manifest allowlisting your exact extension ID. The native host and
   daemon also validate instance, paired profile and configured extension ID.
3. Wait for the native host's value-free ready message. Do not send material
   requests from the extension or expose a website/content-script relay endpoint.
4. Implement `targets` with safe origins and stable tab/frame/document identities.
   Never return full URLs with query credentials, page dumps or input values.
5. On a daemon-authorized `fill`, independently recheck permission, actual page/
   frame origin, document IDs and supported controls. Execute only a fixed function
   in an isolated world targeting that exact document, and return closed statuses.
6. On disconnect or replacement, reject stale work and rebind deliberately. Never
   replay an in-flight fill, persist values in extension storage, or log payloads.

Bridge wire version 1 is separate from agent protocol version 2. See
[the native channel](protocol.md#native-integration-channel) and its Rust
`BridgeCommand`, `BridgeReply`, `BridgeRequest` and `BridgeResult` definitions.
The bridge carries plaintext into trusted code; it is not an agent API. Chrome's
normal identity checks do not stop unrestricted same-user impersonation.

The initial installer supports one configured extension/client profile per user
for this native host name. Multiple extension IDs/independent host installations
are a future packaging decision, not an implicit supported configuration.

## Conformance checklist

Before advertising a browser integration, exercise:

- Correct synthetic field delivery and continued original-tool operation.
- Actual page/frame/document binding, origin restrictions, navigation and expiry.
- Genuine human approval; refusal to infer effect permission from metadata access.
- Strict locators, ambiguous/replaced/unsupported inputs and reactive input events.
- No material in agent arguments, own replies, errors, logs or audit records.
- Partial fill, disconnect, cancellation, lost reply and no automatic retries.
- Install/removal, permission denial, host/worker restart and reconnect.
- Bounded input/output, admission, memory and deadlines; no human waits under
  custody locks and no browser/process ownership theft.

Passing delivery tests does not qualify filtering another tool's screenshots,
DOM, cookies or session artifacts. Do not advertise universal credential secrecy
or protection against arbitrary programmable consumers.

## Existing embedded consumers

Phase 3 changes standalone crates and adds `magicvault-effect`; core `0.1.3` and
primitives `0.1.1` remain unchanged. Magician keeps its direct core integration,
existing store identity and browser execution owner. It does not consume the new
standalone registry, CLI, MCP, extension or native host. MagicRun is not required
for browser fills; it is the intended execution dependency for later new-process
work. Future shared-core changes require deliberate compatibility review rather
than a frozen fork or an implicit runtime migration.
