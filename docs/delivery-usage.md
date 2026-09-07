# New commands and HTTP requests

`secure_new_process` and `secure_new_http` deliver stored credential fields to a
**human-registered, fixed destination**. The agent supplies only a profile UUID
and an operation UUID. The daemon owns consent, resolution, execution and a
value-free receipt. This is not an unrestricted shell or HTTP client.

Source version: **0.4.0 alpha**. Standalone native approval/keychain requires
macOS and an interactive desktop. Headless callers still require human consent.
See [setup](setup.md), [security](../SECURITY.md) and [qualification](qualification/process.md).

## Register once; approve each invocation

Per-use approval is the default. The native use dialog can remember an exact
registered profile with **Always allow**; registration itself never does so.
[Scope and returning to per-use prompts](consent.md).

1. Pair a client and enroll a synthetic credential using the human CLI. If it
   belongs to another client, explicitly approve metadata access first.
2. Copy [process-profile.json](../examples/process-profile.json) or
   [http-profile.json](../examples/http-profile.json). Replace its placeholder
   paths/URL and credential reference/field. Every literal is **non-secret public
   configuration**, displayed in native consent; never paste credentials there.
3. Register using the same paired profile the agent will use:

   ```bash
   magicvault --profile agent register-delivery-profile --request-file /absolute/path/to/profile.json
   magicvault --profile agent list-delivery-profiles
   ```

4. Review the exact recipient, arguments and placements in the native dialog.
   Keep the returned `profile_id`. Registration does not launch anything or
   grant future automatic use. The MCP catalog intentionally has no registration
   or profile-editing tool.
5. Choose an operation ID once, request delivery, approve its separate native
   dialog, and poll the same ID:

   ```bash
   operation_id=$(uuidgen)
   magicvault --profile agent secure-new-process \
     --profile-id REPLACE_WITH_PROFILE_UUID --operation-id "$operation_id"
   # Use secure-new-http for an HTTP profile; do not invoke both for one ID.
   magicvault --profile agent delivery-status --operation-id "$operation_id"
   ```

The corresponding MCP tools use underscores: `list_delivery_profiles`,
`secure_new_process`, `secure_new_http`, `delivery_status`, `cancel_delivery`.
Both invocation tools take exactly `{"profile_id":"UUID","operation_id":"UUID"}`.
They reject command, URL, argument, credential-value and approval overrides.
Discovery returns only your profile IDs, labels and kinds, not another client's
profiles or the full configuration. Profiles are immutable; remove and register
a new one to change any destination or refresh an executable digest.

```bash
magicvault --profile agent cancel-delivery --operation-id "$operation_id"
magicvault --profile agent remove-delivery-profile --profile-id REPLACE_WITH_PROFILE_UUID
```

Removal narrows authority without another human prompt and cancels related jobs.
Client revocation removes its profiles too. Neither action can recall material
or undo an effect already dispatched. Do not infer success from a cancel reply;
inspect the final receipt.

## Reference placements

Each `InputValue` is one of:

```json
{"kind":"literal","value":"non-secret fixed text"}
```

```json
{
  "kind":"credential",
  "credential_ref":"cred_00000000-0000-0000-0000-000000000000",
  "credential_field":"password",
  "prefix":"Bearer ",
  "suffix":""
}
```

Prefix/suffix are optional fixed text, not expressions. There is no environment,
shell, file, URL, recursive JSON or arbitrary template evaluation. The broker
checks that every selected reference and field belongs to the paired client.
Resolution happens only after native per-use or a revalidated human-created exact-use grant and durable authorization audit.

Profiles are bounded to 12 KiB serialized JSON, 80-byte labels, eight distinct
credential fields, 4096-byte literals and 256-byte prefixes/suffixes. Profile
metadata is printable ASCII (text templates also permit tab/CR/LF); enrolled
values are UTF-8 text up to 4096 bytes. Header/environment rules can further
restrict which values are deliverable. Unknown JSON fields are refused.

## Process destinations

The process configuration fixes the absolute `executable`, `arguments`, absolute
`working_directory`, `environment` rows, optional `stdin` value and `timeout_secs`.
No secret can be placed in argv. Stdin is a finite pipe followed by EOF, not an
interactive session. Fixed arguments are passed as individual strings; MagicVault
does not parse shell syntax.

The executable must be a regular, executable, non-symlink file owned by the
current user or root, not group/other writable, and at most 64 MiB. Its BLAKE3
digest is captured before profile consent and rechecked by MagicRun at its own
dispatch fence. Changing executable bytes requires re-registration. Use trusted
directories; the final working-directory component must not be a symlink.

**This is recipient binding, not an OS sandbox or whole-program attestation.**
The interpreter, shared libraries, files named in fixed arguments, configuration
and network destinations chosen by the program are not recursively hashed.
An approved shell, interpreter or extensible CLI is trusted code capable of
copying credentials. Only approve programs and all relevant inputs you trust.

MagicRun's existing governed batch coordinator owns admission, materialization,
dispatch, bounded pipe draining and owned-child cleanup. The standalone adapter
uses no inherited environment: its baseline provides only `PATH` pointing to
the executable's parent and `LANG=C.UTF-8`. Credential placements cannot override
reserved baseline/loader variables. Programs requiring other environment or
ambient login state may not work; register supported non-secret environment
entries explicitly, without assuming the caller's `HOME`, PATH or shell setup.

Limits: 32 fixed arguments of at most 512 bytes each, eight environment rows,
one stdin slot, a 1–120 second runtime deadline and 64 KiB per output stream.
Output is consumed under MagicRun's bounds and **never projected to the agent**.
Cancellation waits for the owned worker's cleanup before releasing broker
admission. A program escaping process-group ownership or changing external state
is outside a rollback guarantee. No PTY, arbitrary PID attachment, credential
file mounting, persistent service ownership or memory-isolation jail is enabled.

## HTTP destinations

The HTTP configuration fixes `url`, `method`, `headers`, `query`, optional `body`
and a 1–120 second `timeout_secs`. URL userinfo, fragments and embedded query
strings are refused; use explicit `query` rows for non-secret or credential query
values. Headers and query/body rows use `{"name":"...","value":InputValue}`.

- **HTTPS:** public IP destinations or domain names whose resolved addresses are
  all permitted public addresses. Native trust roots and normal TLS hostname
  verification apply. Private/internal HTTPS, custom CA/client certificates and
  caller-defined resolver/proxy settings are not supported.
- **Plain HTTP:** only explicit loopback IP literals such as `http://127.0.0.1:PORT/`
  or `http://[::1]:PORT/`, for trusted local development recipients. `http://localhost`
  is refused. Loopback is unencrypted; another local listener is still a recipient
  you must trust. Use synthetic credentials in local qualification.
- **Methods:** uppercase ASCII names/hyphens, 1–16 bytes. Standard methods and
  custom methods such as `PROPFIND` work; `CONNECT` is refused. No automatic
  redirect, retry, cookie jar, Referer, ambient proxy or insecure TLS fallback.
- **Placements:** headers, URL-encoded query values, text body, URL-encoded form
  rows or a flat JSON object of string-valued rows. No multipart, binary/streaming
  upload, nested JSON, pagination or response-dependent follow-up request.
- **Transport headers:** duplicate header names (case-insensitive), Host, framing,
  connection, upgrade and proxy overrides are refused. Body kind selects a single
  Content-Type; do not also configure that header. CR/LF in a resolved header is
  rejected before dispatch. Query credentials can enter a recipient's access logs;
  prefer a header where the API supports it.

DNS results are vetted and pinned for that request while preserving TLS hostname
verification. At most 16 addresses are accepted, with a five-second resolver
wait within the overall deadline. The OS resolver is not cancellable: at most
two credential-free resolver threads may remain outstanding across all callers;
their permits remain held until completion. Further resolution fails `busy`
instead of accumulating threads. Those threads do not hold daemon shutdown open.

HTTP response bodies are discarded incrementally up to 1 MiB. No body, response
header, URL, raw HTTP status code or transport diagnostic is returned or audited.
This intentionally also withholds encoded/transformed credential echoes.

## Receipts, concurrency and uncertainty

| State | Interpretation |
| --- | --- |
| `pending` | Awaiting exact-use human consent; no dispatch yet |
| `running` | Dispatch may occur or may already have occurred; continue polling |
| `completed` | Process completed successfully, or a 2xx HTTP exchange completed within bounds; no business-success or login guarantee |
| `denied`, `cancelled`, `expired`, `failed` | Inspect `may_have_run`; a failed recipient response can follow a real side effect |
| `uncertain` | Dispatch, transport, cancellation or final durable audit could not establish a clean outcome; reconcile, never automatically retry |

`may_have_run=false` is explicit evidence from that retained operation, not a
claim about a missing result. Timing and coarse success/failure are still
observable; this is content withholding, not information-theoretic secrecy.

The shared human/effect gate permits one active workflow through final cleanup
and audit. Other attempts get `busy`; there is no unbounded prompt queue. There
are at most 16 durable profiles and 32 retained delivery jobs per daemon. Detailed
results expire ten minutes after admission (unfinished jobs are retained), while
up to 4096 spent operation IDs remain refused for the daemon epoch. An identical
retained invocation returns its original receipt; changed bindings conflict.
No client automatically retries a mutation.

Profiles survive restart; jobs do not. Old epochs fail closed. Missing status,
including after restart, **does not prove the recipient was untouched**. Durable
closed audit receipts help a trusted operator reconcile, but are not a distributed
exactly-once guarantee. A final audit failure makes the result uncertain, blocks
new work, and preserves authenticated `delivery_status` while retained.

Browser connections remain the separate [secure-fill](browser-usage.md) path.
Already-running services need an explicit cooperative refresh/provider protocol;
new-process delivery does not change another process's environment or update an
existing HTTP connection pool.
