# Local protocol and trust boundary

The daemon owns one `SecretStore` and a process-held exclusive instance lease.
Magician independently embeds the same core in its own root with its existing
identity; it does not adopt this service or its policy/profile files.

The Unix socket is `ROOT/rpc.sock`, mode 0600 under a private owned directory.
Both sides verify peer effective UID. A random 256-bit paired capability further
selects the client; the registry stores only its SHA-256 digest. Bootstrap pairing
is same-user plus a real daemon-owned native decision. MCP cannot invoke it.

**Limit:** same-user unrestricted code can read pairing files, impersonate a
client, drive native UI if it has automation authority, debug trusted processes,
or access an authorized keychain. This is not a sandbox against that code. The
supported agent channels keep enrolled credential values out of requests,
responses, logs and diagnostics. Client labels/field names are intentionally
non-secret human-authored metadata. Paired capabilities are transport secrets,
not enrolled credential values, and must not be copied into agent messages.

## Frames and messages

One request/reply per connection: unsigned 32-bit little-endian byte length,
then UTF-8 JSON. Requests are limited to 32 KiB; replies to 256 KiB; 16 admitted
connections, five-second frame reads/writes, bounded native interaction, and no
automatic mutation retries. The SDK MCP transport separately limits inbound
messages to 32 KiB and replies to 1 MiB, and routes at most eight active tool calls.
Its writes/close have five-second deadlines; a partial-write failure closes the
writer without retry. The registry is bounded to 32 clients × 256 references
and 1 MiB of serialized state including at most 64 browser-permission rows.
Enrollment allows eight 4096-byte text fields.
Native consent metadata is bounded to 16 KiB so valid browser rules and escaped
selectors are not silently truncated. Secret answers remain bounded to 4096 bytes.
After SDK completion the MCP client bounds runtime teardown to 250 ms so an
uncancellable Tokio stdio task cannot indefinitely hold the process open. This
client owns no store writes; the custody daemon does not use bounded teardown.

Envelope fields: `version: 2`, UUID `request_id`, daemon `epoch` (from status),
optional pairing `token`, and a tagged `request` with `method` / optional `params`.
Every non-status request binds the current epoch. No caller sends a grant decision
or plaintext vault field through this protocol. Parse/OS/keychain errors use
closed codes; raw diagnostic text and process output are never replies.

| Request | Caller/result |
| --- | --- |
| `status` | Same-user health; with valid token also own client ID; effects contains `secure_fill` |
| `pair {label}` | Human CLI bootstrap; native consent; private pairing capability saved by client, never printed |
| `enroll {label, field_names}` | Paired human CLI; native consent and hidden inputs; metadata-only result |
| `list_credentials` | Paired client; only explicitly permitted credential metadata |
| `request_access {credential_ref}` | Paired client; asynchronous human metadata-consent request |
| `approval_status {approval_id}` | Only the requesting client; pending/allowed/denied/expired/uncertain |
| `revoke_client {client_id}` | Paired CLI plus native consent; invalidates future use of that pairing |
| `shutdown` | Paired CLI plus native consent; cancels prompts and drains ownership |
| `register_cdp {label, endpoint}` | Paired CLI plus native consent; loopback browser endpoint; client-owned handle |
| `configure_browser_credential {credential_ref, origins, field_names}` | Paired CLI plus native consent; explicit browser permission, not metadata consent |
| `list_browsers` | Paired client; only own connected browser handles |
| `browser_targets {browser_handle}` | Paired client; safe origins and backend IDs plus single-use document-bound handles |
| `disconnect_browser {browser_handle}` | Owning paired client; cancels jobs, closes integration only |
| `secure_fill {operation_id, browser_handle, target_handle, fields}` | Paired client; consumes target, returns pending status; daemon-owned exact-use consent then delivery |
| `fill_status {operation_id}` | Only owning paired client; metadata-only status, also available after persistence uncertainty |
| `cancel_fill {operation_id}` | Owning paired client; cancellation request, never a rollback claim |

MCP translates `request_approval` to `request_access`, and `vault_status` to
`status`. Its catalog is a fixed subset, not automatic exposure of every request.
Use the [official SDK](https://github.com/modelcontextprotocol/rust-sdk) for MCP
protocol/lifecycle; no custom MCP wire protocol is implemented here. Native hidden
input uses [Apple's documented dialog facility](https://developer.apple.com/library/archive/documentation/LanguagesUtilities/Conceptual/MacAutomationScriptingGuide/PromptforText.html)
with a fixed script, metadata argv, private bounded answer pipe and discarded stderr.

## Persistence and cancellation

`instance.json` is a versioned UUID identity; the keychain uses service
`ai.magicbeans.magicvault`, account `instance-UUID`, never Magician's names.
The daemon caches the loaded key for its lifetime. `clients.json` contains
versioned paired-client hashes, metadata ACLs and explicit standalone browser
permissions. Core vault bytes stay in
`ROOT/vault`, using existing encryption/partition formats. No key/vault import,
replacement, migration, automatic data cleanup or ambient credential discovery.
Initialization syncs the parent entry naming the private root before creating
its key/identity. Shared durable byte writers create staging files exclusively
with the requested mode before any payload, sync data and permissions, rename,
then sync the destination parent. Bare relative filenames use `.` as that parent.

Standalone decisions use the opt-in `try_audit_event_durably` core method: append,
sync the journal, sync its directory, then sync the parent naming that directory,
all under the journal lock and the broker's serialized writer. A failed append or
sync poisons the broker and prevents an optimistic successful response. The
pre-existing `audit_event` / `try_audit_event` methods remain append-only; Magician
does not incur the standalone audit barriers. The instance root must already be
durably established, as it is by explicit initialization. Startup also refuses
symlinked, non-regular, foreign-owned or non-private existing audit journals
before core can append to or tighten their permissions.

Enrollment persists the core entry before publishing client metadata permission.
An uncertain second write may leave an encrypted entry without an ACL; the
service fails closed. Startup does not infer a grant from such an orphan.
Requests use deterministic enrollment IDs to refuse same-ID overwrites. There is
no safe-to-retry claim after a transport/persistence failure.
Enrollment revalidates its monotonic deadline after waiting for the serialized
writer and after audit, immediately before writing the core entry.
Standalone startup refuses quarantine evidence, mismatched ACL/material state,
unsafe file modes and oversized vault files rather than silently creating an
empty ready service. This host policy does not change Magician's core recovery
behavior. Core metadata projection clones only names, not field values/lengths.

Human waits hold no store lock. Completion rechecks caller validity, shutdown,
expiry and target-reference presence before allowing access. Native interaction
is serialized; pending jobs/results are bounded. Results are ephemeral across
restart, while completed metadata ACLs persist. They are not future effect grants.
Within the retained job window, replay lookup and reservation are atomic: the
same request ID/owner/reference returns its original status without a new prompt;
changing that binding returns `conflict`. This does not authorize automatic
retries after lost transport replies or make other mutations idempotent.

Shutdown cancels prompts, stops admission, drains connections/human work, and
retains the instance lease through outstanding blocking commits. A second owner
cannot race a still-running write. The service must be requalified before it is
deployed over valuable credentials. Only [targeted synthetic fixtures](targeted-tests-2026-09-06.md)
have run on earlier revisions, not current Phase 3 or native-host/full regression
qualification. See the current [coverage and execution ledger](testing.md).

## Browser effects

The fixed `fields` entries contain `css`, `credential_ref` and
`credential_field`, never values. Request parsing rejects unknown fields, caller
decisions and JavaScript. CSS selectors are bounded to 512 printable ASCII bytes
(use CSS escapes for Unicode identifiers); at most eight
fields are accepted. `operation_id` must be a non-nil UUID selected once by the
caller, independent of the transport request ID. No client retries effects.

Browser registration, permissions, target discovery and effect jobs are owned by
the paired client. Targets contain the actual backend tab/frame/document identity,
top-document identity, origins and a monotonic 180-second expiry. Public discovery
omits full URLs, page titles and field values. A request-supplied identity is not
authority: it must resolve to that client's daemon-issued bound handle.

There are eight browser connections, 128 unused targets and 32 retained fill
results per daemon. Results expire ten minutes after admission, except unfinished
jobs. Up to 4096 spent operation IDs remain tombstoned until daemon restart;
capacity exhaustion fails closed rather than dropping replay protection. Each
backend has bounded non-queuing admission. Native human work is serialized and a
fill retains that permit through its completion/audit, without holding a store
lock while waiting on human/browser I/O. Browser I/O has a 30-second operation
deadline. The CDP connection caps messages/frames at 512 KiB and command waits at
five seconds. No protocol or library transport diagnostics are model results.

Browser permission matches exact origins for **both** top page and selected
frame, and the selected credential fields. It starts absent for existing entries.
The native decision then covers one requested effect; the service revalidates
caller, permission, deadline, cancellation, references and browser existence after
consent and durable authorization audit, immediately before releasing material.
Adapters revalidate document/origin and selectors at the browser boundary.

The fill handle is consumed at admission. Identical retained requests return the
original status; changed bindings conflict. After the detailed result expires,
the operation ID remains spent. This is not a distributed exactly-once promise:
the browser can apply a write before the reply is lost, and restart removes
ephemeral result state. Never repeat an uncertain operation under a new identity.

Completion records a typed metadata-only audit receipt containing operation,
client, browser and target IDs plus overall/per-field status and a closed error.
No selector, raw URL, value, page dump or browser diagnostic is journaled.
If post-effect audit fails, the final state is `uncertain`, new work is blocked,
and authenticated `fill_status` remains available for reconciliation while that
result is retained. A receipt describes delivery, not successful authentication.

## Native integration channel

`ROOT/bridge.sock` is a separate same-user, private Unix socket. It is never
forwarded through MCP or the generic CLI response path. A native host presents
bridge version 1, instance UUID, configured extension ID and paired capability.
The daemon validates the configured profile/extension, authenticates the peer and
asks the human to connect it. No credential material appears in that handshake.

After handshake, the daemon sends `BridgeCommand {request_id, request}` with
`targets` or a trusted `fill {target, fields}`. Here—and only in the trusted
browser channel—each field carries CSS plus its value. The reply is a closed
`BridgeReply {request_id, result}` with target metadata, per-field outcome, or
closed discovery error. The host validates IDs and response kind; values cannot
be represented by the reply schema. Frames use a native/Unix 32-bit little-endian
length and UTF-8 JSON, bounded to 256 KiB. Unknown fields and wrong IDs close the
channel. Partial frames, cancellation and disconnect are never retried.

Chrome's allowed-origin manifest plus the host's invocation-origin/config checks
bind normal extension connections. This is not protection from arbitrary
same-user impersonation. The extension service worker owns native messaging;
it uses document-targeted isolated script execution, not a page message route.
It stores no credential material and exposes only setup/status to its own
extension page. Approved reconnect replaces the previous extension channel and
invalidates unused handles. User reconnection is explicit; no effect is replayed.
