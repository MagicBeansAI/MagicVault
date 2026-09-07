# Consent every time or Always allow

The daemon-owned credential-use dialog offers **Deny**, **Allow once**, and
**Always allow**. Per-use consent is the default; Deny remains the default/cancel
button. Neither CLI nor MCP can submit an allow decision.

- **Allow once:** authorize only this operation; the next new use asks again.
- **Always allow:** authorize this operation and remember its exact scope for
  this paired client. Matching future uses skip the native use prompt.
- **Deny:** do not authorize this operation or create a remembered grant.

Pairing, enrollment, credential policy, destination registration and administration
retain separate native decisions. Extension website permission—including **Allow
all HTTPS websites**—never creates credential-use consent.

macOS Keychain's **Always Allow** is separate too: it permits the application to
access its Keychain item. It does not create a MagicVault credential-use grant or
approve a browser, process or HTTP destination.

## Exact scope and lifetime

| Surface | Binding | Lifetime |
| --- | --- | --- |
| New process / HTTP | Paired client and immutable profile ID, fixing all credential references/placements and executable/arguments/cwd or URL/method; executable digest is still checked | Until consent/profile/client revocation; survives daemon restart |
| Native extension | Paired client, extension ID, browser profile ID, exact top/frame origins including ports, main-frame versus iframe, complete ordered credential-field/CSS mapping | Until consent/client/browser revocation or relevant credential-policy reconfiguration; survives reconnect/restart |
| CDP / trusted custom adapter | Same browser scope, but tied to its current live browser handle | Forgotten on disconnect or daemon restart |

A browser grant is **not restricted to one tab, URL path or document**. New
documents and same-origin frames matching the approved scope can reuse it.
Use per-use consent when those distinctions matter. Every operation still needs
a fresh daemon-issued target and passes live document/control/origin/site checks.
No grant authorizes form submission or another tool to read values.

Registering another process/HTTP profile—even identical configuration—requires
a separate use decision. An executable digest is not recursive attestation of
interpreters, libraries or input files; trust the whole recipient. See
[SECURITY.md](../SECURITY.md).

## Return to consent every time

Use the same root and paired profile as the requesting agent:

```bash
magicvault --profile agent list-consents
magicvault --profile agent revoke-consent --grant-id GRANT_UUID
# Or forget every remembered use owned by this paired client:
magicvault --profile agent clear-consents
```

Listing returns IDs, labels and reference-only scopes, never values. Revocation
needs no approval because it removes authority. A single revocation cancels
matching pending/running effects; clearing cancels all this client's browser and
delivery jobs. Resetting also invalidates pending native decisions, so a late
Always allow answer cannot recreate consent. Other clients' grants remain private.

Already delivered work cannot be recalled. Removing Chrome permission or blocking
a site prevents browser delivery but does not delete the separate use grant;
restoring site access may make it usable again. Revoke the grant itself to forget it.

## Persistence, limits and recovery

Grants contain no values and live in the private standalone registry. The instance
admits at most **16 remembered uses**, each bounded to 12 KiB. The registry is capped
at 1.25 MiB; all list replies fit the existing 256 KiB limit. Over-capacity Always
allow fails closed, never silently broadens scope or degrades to an unrecorded mode.

Grants and revocations are durably written and audited. Failed persistence faults
the daemon and prevents new effects. An uncertain write proves neither creation
nor revocation: reconcile with `doctor` and `list-consents` after recovery. Do not
delete state or replay a missing/uncertain effect. Always allow does not change
single-use operation IDs, cancellation, output withholding or no-retry semantics.

Version `0.8.1` corrects pending process/HTTP cancellation receipts: cancelling
the native prompt reports `cancelled`, while a human Deny remains `denied`.
An explicit expiry remains `expired`. The older receipt bug did not dispatch
credentials or create a grant; [native qualification](qualification/results-cancellation-2026-09-08.md)
records the distinction and its verification.

An explicitly chosen Always allow grant can remain even if that operation later
expires or fails at the recipient. A failed effect does not undo the human's
remembered decision; inspect and revoke the grant to return to per-use consent.

## Compatibility and acceptance

The `0.8.1` standalone bundle uses agent wire **4**: update CLI, MCP, daemon and
native host together. Extension `0.6.1` changes explanatory text, not its bridge
or profile handshake. Existing registries default to no remembered use. Older
executables can reject saved grant fields; never strip them to force a downgrade.
Shared custody core, primitives, MagicRun and Magician are unchanged.

Synthetic tests cover scope, persistence, revocation, late decisions and failure
boundaries. Genuine native selections and recovery are separate acceptance gates:
see the [process/HTTP](qualification/process.md) and
[extension](qualification/extension.md) runbooks. Prior `0.7.0` qualification does
not qualify this new policy.
