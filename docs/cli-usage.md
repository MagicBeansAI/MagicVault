# CLI for agents and scripts

Use the CLI when your agent has shell access but no MCP support, or when you want
explicit Bash/tool-wrapper calls. MCP is not required. The same paired daemon,
destination policy and native per-use or explicitly remembered consent apply: a script is **not** an
unattended/headless-CI credential runner. The CLI also contains human administration
commands; scripts must not try to grant themselves approval.

See [consent modes](consent.md) for exact scopes and lifetimes. Human administration
uses `list-consents`, `revoke-consent --grant-id GRANT_UUID`, or `clear-consents`
to inspect or remove remembered uses. These commands cannot grant approval.

| Operation | MCP tool | CLI command |
| :--- | :--- | :--- |
| Fill an existing browser field | `secure_fill` | `magicvault secure-fill` |
| Launch a registered new command | `secure_new_process` | `magicvault secure-new-process` |
| Send a registered new HTTP request | `secure_new_http` | `magicvault secure-new-http` |

After [human setup and enrollment](setup.md), inspect value-free metadata:

```bash
magicvault --profile agent status
magicvault --profile agent list-credentials
magicvault --profile agent list-delivery-profiles
export MAGICVAULT_EXAMPLES="$HOME/.magicvault-app/current/examples"
```

Source builders can use their checkout's `examples/` instead. Recipient output is
withheld; capture closed receipts and retain each operation ID for reconciliation.

## Browser fill from the CLI

```bash
magicvault --profile agent list-browsers
magicvault --profile agent browser-targets --browser-handle REPLACE_WITH_BROWSER_HANDLE \
  --top-origin https://accounts.example.com
```

Replace the example origin with the intended exact page origin, including its
port for a local fixture. Optional `--tab-id` narrows further; omit both filters
only for a bounded all-target listing. Filters do not grant permission. On
`capacity`, narrow discovery rather than changing grants or closing tabs.

Choose the correct tab/frame from discovery. Copy the reference-only example to
a temporary file and edit it with your credential reference, browser/target
handles, an exact CSS selector, and a **fresh operation UUID** from `uuidgen`:

```bash
fill_request=$(mktemp /tmp/mv-fill.XXXXXX)
cp "$MAGICVAULT_EXAMPLES/secure-fill.json" "$fill_request"
uuidgen
open -e "$fill_request"
```

Save the file before continuing. It contains references and a selector, never
the password. For the local demo's password input, use `#password` as the selector.

```bash
magicvault --profile agent secure-fill --request-file "$fill_request"
magicvault --profile agent fill-status --operation-id REPLACE_WITH_OPERATION_UUID
```

Approve the fill's native dialog and poll that **same** operation for completion.
Target handles expire after three minutes; discover them after preparing your
test page and credential. `filled` means delivery, not successful login. Your
existing browser tool handles the next step. Never automatically repeat a
`partial` or `uncertain` operation. [Status, cancellation and recovery](browser-usage.md#request-a-fill-and-inspect-its-outcome).

## New commands and HTTP requests

After pairing/enrollment, a human registers a fixed destination using the same
paired client profile. Copy **one** reference-only example and edit its paths or
URL and `credential_ref`/field. Do not insert a password into the file:

```bash
delivery_profile=$(mktemp /tmp/mv-delivery.XXXXXX)
cp "$MAGICVAULT_EXAMPLES/http-profile.json" "$delivery_profile"  # Or process-profile.json.
open -e "$delivery_profile"
# Save after reviewing the exact recipient and every credential placement.
magicvault --profile agent register-delivery-profile --request-file "$delivery_profile"
magicvault --profile agent list-delivery-profiles

operation_id=$(uuidgen)  # Choose once; keep this ID for reconciliation.
magicvault --profile agent secure-new-http \
  --profile-id REPLACE_WITH_REGISTERED_PROFILE_UUID --operation-id "$operation_id"
# For a process profile, use secure-new-process with the same two flags.
magicvault --profile agent delivery-status --operation-id "$operation_id"
```

Registration requests native consent. Invocations need separate native consent unless the human has remembered that exact use. The agent can
select a profile ID, **not override its destination**. `completed` is a transport/
process receipt, not proof of application success. Output is deliberately withheld,
including encoded credential echoes. A missing or uncertain result never means
safe to retry. [Profile format, limits, cancellation and recipient trust](delivery-usage.md).
