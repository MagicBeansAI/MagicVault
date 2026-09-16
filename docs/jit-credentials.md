# One-time credentials during browser automation

Use `secure_prompt_fill` when an automation reaches a login page and no saved
credential is appropriate. MagicVault asks you for the values in native hidden
inputs, then asks **Use once** for the exact browser, page/frame origins and field
mapping. It fills those fields and returns status. Nothing is enrolled in the
vault, and no future-use permission is created or reused.

This flow is available in the `0.9.0` source candidate. The existing recorded demo
shows saved credentials with `0.8.3`; it does not demonstrate this new flow.

## Before the first use

Complete [setup and pairing](setup.md), run the daemon and connect a browser by
[CDP or extension](browser-usage.md). Use the same paired client for browser setup
and MCP/SDK calls. Extension site permissions and browser-profile approval still
apply. There is no enrollment or `configure-browser-credential` step for one-time
input: the native input and final confirmation authorize this single delivery.

## MCP and CLI

1. Navigate to the login page using your existing browser tool.
2. Call `list_browsers`, then `browser_targets` for that browser. Narrow by exact
   `top_origin` and, when needed, backend `tab_id`; select the intended frame.
3. Call `secure_prompt_fill` with a fresh operation UUID, the discovered handles
   and non-secret field names/selectors:

```json
{
  "operation_id": "11111111-1111-4111-8111-111111111111",
  "browser_handle": "22222222-2222-4222-8222-222222222222",
  "target_handle": "33333333-3333-4333-8333-333333333333",
  "fields": [
    { "css": "#username", "field_name": "username" },
    { "css": "#password", "field_name": "password" }
  ]
}
```

Replace these example handles with discovery results. To use the CLI, put only
this metadata in a private JSON file and run:

```sh
magicvault --profile agent secure-prompt-fill --request-file request.json
magicvault --profile agent fill-status --operation-id OPERATION_UUID
# To cancel explicitly:
magicvault --profile agent cancel-fill --operation-id OPERATION_UUID
```

Enter values only in the native windows, never in chat, JSON or shell arguments.
Every input is hidden, including usernames. **Continue** collects the next field;
the final **Use once** authorizes delivery. **Cancel** discards the collected inputs
without dispatch. No save option is offered. Poll `fill_status` with the original
operation ID; the existing browser tool handles submission after `filled`.

## TypeScript

The [Node SDK](typescript.md) provides the same operation:

```ts
import { MagicVault, createOperationId } from '@magicvault-local/magicvault';

const vault = new MagicVault({ profile: 'agent' });
// browser and target are selected from listBrowsers()/browserTargets().
const operation_id = createOperationId();
await vault.securePromptFill({
  operation_id,
  browser_handle: browser.browser_handle,
  target_handle: target.target_handle,
  fields: [
    { css: '#username', field_name: 'username' },
    { css: '#password', field_name: 'password' },
  ],
});
const receipt = await vault.waitForFill(operation_id, { timeoutMs: 210_000 });
console.log(receipt.state); // Delivery status only; inspect before submission.
```

## Lifetime and boundary

- The request accepts 1–8 fields with distinct selectors and names. Values are
  nonempty single-line UTF-8, at most 4096 bytes each. Caller-provided values,
  references, consent decisions and save/remember flags are rejected.
- Collection and confirmation must finish before the original target's
  180-second expiry. Browser dispatch has its existing bounded deadline. A poll
  timeout does not cancel the daemon operation; use `cancel_fill` and reconcile.
- One operation consumes its target handle and shares replay protection with
  saved fills. Never automatically retry after a lost reply or `uncertain`
  outcome. Missing status after restart is not evidence that nothing happened.
- Owned temporary input/material buffers are zeroized on drop. MagicVault does
  not put the values in its credential store, registry, audit records, request
  files or agent replies. Receipts contain only IDs and closed field/status codes.
- The destination website receives the values. Other browser tools, screenshots,
  same-user software and native/browser memory remain outside this guarantee.
  This is not proof of complete OS/browser memory erasure or recipient deletion.
- The flow is browser-only. HTTP/process profiles still use enrolled references.
  Existing saved credentials and remembered-use consent retain their own path.

## Integrations and validation

MagicRun needs no changes: browser fills use MagicVault's existing adapters.
An MCP consumer can discover the new tool after the matching MagicVault bundle
is upgraded and its MCP connection restarted. Magician has a separate source
integration, `browser__secure_prompt_fill`, using its own private HITL prompts
and the shared browser adapter. It requires rollout of the matching Magician
runtime, UI, Magicutor and browser skill; this standalone upgrade does not enable
it automatically. That integration uses no standalone MagicVault MCP connection.
See [embedded consumers](integrations.md#existing-embedded-consumers).

Automated checks exercise empty-vault delivery, cancellation during collection
and confirmation, expiry, revocation, changed documents, replay, partial/uncertain
outcomes and value-free persistence/results. CLI, MCP and SDK checks use real
native binaries and authenticated IPC with synthetic human/CDP peers. They do not
qualify the new native windows in an installed browser session; that remains a
separate acceptance step.
