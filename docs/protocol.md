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
and 512 KiB of serialized state. Enrollment allows eight 4096-byte text fields.

Envelope fields: `version: 1`, UUID `request_id`, daemon `epoch` (from status),
optional pairing `token`, and a tagged `request` with `method` / optional `params`.
Every non-status request binds the current epoch. No caller sends a grant decision
or plaintext vault field through this protocol. Parse/OS/keychain errors use
closed codes; raw diagnostic text and process output are never replies.

| Request | Caller/result |
| --- | --- |
| `status` | Same-user health; with valid token also own client ID; effects list is empty |
| `pair {label}` | Human CLI bootstrap; native consent; private pairing capability saved by client, never printed |
| `enroll {label, field_names}` | Paired human CLI; native consent and hidden inputs; metadata-only result |
| `list_credentials` | Paired client; only explicitly permitted credential metadata |
| `request_access {credential_ref}` | Paired client; asynchronous human metadata-consent request |
| `approval_status {approval_id}` | Only the requesting client; pending/allowed/denied/expired/uncertain |
| `revoke_client {client_id}` | Paired CLI plus native consent; invalidates future use of that pairing |
| `shutdown` | Paired CLI plus native consent; cancels prompts and drains ownership |

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
versioned paired-client hashes and metadata ACLs. Core vault bytes stay in
`ROOT/vault`, using existing encryption/partition formats. No key/vault import,
replacement, migration, automatic data cleanup or ambient credential discovery.

Enrollment persists the core entry before publishing client metadata permission.
An uncertain second write may leave an encrypted entry without an ACL; the
service fails closed. Startup does not infer a grant from such an orphan.
Requests use deterministic enrollment IDs to refuse same-ID overwrites. There is
no safe-to-retry claim after a transport/persistence failure.
Standalone startup refuses quarantine evidence, mismatched ACL/material state,
unsafe file modes and oversized vault files rather than silently creating an
empty ready service. This host policy does not change Magician's core recovery
behavior. Core metadata projection clones only names, not field values/lengths.

Human waits hold no store lock. Completion rechecks caller validity, shutdown,
expiry and target-reference presence before allowing access. Native interaction
is serialized; pending jobs/results are bounded. Results are ephemeral across
restart, while completed metadata ACLs persist. They are not future effect grants.

Shutdown cancels prompts, stops admission, drains connections/human work, and
retains the instance lease through outstanding blocking commits. A second owner
cannot race a still-running write. The service must be requalified before it is
deployed over valuable credentials; the current implementation has not been run.
