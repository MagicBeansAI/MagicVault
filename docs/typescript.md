# MagicVault for Node and TypeScript

Use saved credentials through references, or request one-time browser credentials
through native hidden prompts while your application is running. The npm
package includes a small client, TypeScript declarations, CLI/MCP launchers and
prebuilt Rust binaries for every supported platform in the same package. Users of
these packages do not need Cargo. The SDK uses Node built-ins only.

Install **`@magicbeansai/magicvault`** from npm. Runtime support remains
Node 22+ and an interactive desktop for native consent. See
[platform prerequisites and validation](platforms.md) for macOS, Linux and Windows.
From 0.9.1 the launcher selects a bundled OS/CPU binary. There are no optional
platform dependencies, install scripts or binary downloads.
This is a Node API, not a browser bundle or an unattended CI credential store.

## Install and prepare

Install the main package in your project:

```bash
npm install @magicbeansai/magicvault
npx magicvault --profile agent setup
npx magicvault --profile agent doctor
```

Keep the daemon running and connect a [browser](browser-usage.md) for one-time
fills. To use saved credentials, enroll through the CLI and authorize a
[browser origin](browser-usage.md) or register a fixed [HTTP/process destination](delivery-usage.md). Installing/importing the npm package does not perform
setup, initialize a vault, enroll values, start services or grant consent.

## Request one-time browser credentials

No enrollment is needed. Navigate with your existing browser tool, select the
intended browser and discovered document, then:

```ts
import { MagicVault, createOperationId } from '@magicbeansai/magicvault';

const vault = new MagicVault({ profile: 'agent' });
const browsers = await vault.listBrowsers();
if (browsers.length !== 1) throw new Error('Select the intended browser.');
const browser = browsers[0];
const targets = await vault.browserTargets({
  browser_handle: browser.browser_handle,
  top_origin: 'https://accounts.example.com',
});
const matching = targets.filter(t => t.is_main_frame && t.origin === 'https://accounts.example.com');
if (matching.length !== 1) throw new Error('Select the intended tab before filling.');
const operation_id = createOperationId(); // Retain before dispatch.
await vault.securePromptFill({
  operation_id,
  browser_handle: browser.browser_handle,
  target_handle: matching[0].target_handle,
  fields: [
    { css: '#username', field_name: 'username' },
    { css: '#password', field_name: 'password' },
  ],
});
const receipt = await vault.waitForFill(operation_id, { timeoutMs: 210_000 });
console.log(receipt.state);
```

The user enters values only in native windows and approves **Use once**. Values
are never saved in MagicVault or returned to your code. The SDK's temporary
request file contains only names, selectors and IDs. Use `cancelFill` to cancel;
a polling timeout alone does not cancel delivery. [Lifetime, replay and security
boundaries](jit-credentials.md).

## Send a registered HTTP request

Enroll the required credential and register a profile labeled `Demo API` through the CLI first. It fixes the URL,
method and credential placements; SDK invocations only select its ID.

```ts
import { MagicVault, MagicVaultError, createOperationId } from '@magicbeansai/magicvault';

const vault = new MagicVault({ profile: 'agent' });
const profiles = await vault.listDeliveryProfiles();
const api = profiles.find(p => p.label === 'Demo API' && p.kind === 'http');
if (!api) throw new Error('Register the Demo API destination first.');

const operation_id = createOperationId();
// Persist this non-secret ID before sending if your application must survive restarts.
try {
  await vault.secureNewHttp({ profile_id: api.profile_id, operation_id });
  const receipt = await vault.waitForDelivery(operation_id);
  console.log(receipt.state); // completed, denied, failed, uncertain, etc.
} catch (error) {
  if (!(error instanceof MagicVaultError)) throw error;
  console.error(error.code, error.operationId ?? operation_id);
  // Reconcile using deliveryStatus(operation_id). Do not automatically send again.
}
```

`secureNewProcess` takes the same request shape for a process profile. Neither
method accepts an arbitrary URL/command, credential value, or consent decision.
Replies contain neither HTTP content nor process stdout/stderr. `completed`
describes delivery/transport, not remote application success.

## Fill an existing browser field

Connect your Chromium browser through CDP or the extension and authorize the exact
origin/credential field first. Navigate using your existing browser tool, then:

```ts
import { MagicVault, createOperationId } from '@magicbeansai/magicvault';

const vault = new MagicVault({ profile: 'agent' });
const account = (await vault.listCredentials()).find(c => c.label === 'Demo account');
const browsers = await vault.listBrowsers();
if (!account || browsers.length !== 1) throw new Error('Select the intended account and browser.');
const browser = browsers[0];
const targets = await vault.browserTargets({
  browser_handle: browser.browser_handle,
  top_origin: 'https://accounts.example.com', // Your exact authorized test origin.
});
const matching = targets.filter(t => t.is_main_frame && t.origin === 'https://accounts.example.com');
if (matching.length !== 1) throw new Error('Select the intended tab before filling.');

const operation_id = createOperationId(); // Retain this ID before dispatch.
await vault.secureFill({
  operation_id,
  browser_handle: browser.browser_handle,
  target_handle: matching[0].target_handle,
  fields: [{ css: '#password', credential_ref: account.credential_ref, credential_field: 'password' }],
});
const receipt = await vault.waitForFill(operation_id);
console.log(receipt.state); // filled means field delivery, not a successful login.
```

The SDK sends a private temporary JSON file containing references/locators to the
native CLI and removes it afterward. Target handles expire and are single-use.
For multiple tabs, narrow by the backend `tab_id` or select from metadata; never
silently choose the first tab. The original browser tool handles submission.

## Interface

CommonJS is also supported:

```js
const { MagicVault, createOperationId } = require('@magicbeansai/magicvault');
```

| Methods | Result |
| --- | --- |
| `status()` | Readiness, daemon epoch and implemented effects |
| `listCredentials()` | References, labels and field names |
| `requestApproval(ref)`, `approvalStatus(id)` | Native metadata-visibility request/status |
| `listBrowsers()`, `browserTargets(query)` | Connected browsers and short-lived document handles |
| `securePromptFill(request)`, `secureFill(request)` | One-time native input or saved-reference fill; closed receipt |
| `fillStatus(id)`, `cancelFill(id)` | Status or explicit cancellation for either fill mode |
| `listDeliveryProfiles()` | Registered destination IDs, labels and kinds |
| `secureNewHttp(request)`, `secureNewProcess(request)` | Closed delivery receipt |
| `deliveryStatus(id)`, `cancelDelivery(id)` | Closed delivery receipt |
| `waitForFill(id, options)`, `waitForDelivery(id, options)` | Any terminal receipt; inspect its state |

The constructor accepts a paired `profile`, an absolute `root`, an optional
absolute `executable` and a per-call `timeoutMs` (default 10 seconds). By default
it resolves and verifies the matching packaged native executable. An explicit
executable is a host-trusted override; it does not receive npm integrity checks.
Each call starts one native CLI process; this first SDK favors compatibility with
the existing authenticated client over in-process bindings or a new transport.

Calls accept `{ signal }`. Poll helpers additionally accept `{ timeoutMs,
intervalMs }`, defaulting to 60 seconds total and 250 milliseconds between reads.
Only status reads repeat. There is no automatic delivery retry or automatic
daemon cancellation on timeout/abort. Use an explicit cancellation method, then
poll to reconcile effects that may already have happened. A lost reply reports
`transport_uncertain`; it preserves the operation ID. `not_found` after restart
does not mean the delivery is safe to replay.

The SDK validates request shapes, bounds subprocess output and accepts closed
response schemas/error codes. It does not attach raw child errors/streams to
exceptions. It exposes no enrollment, grant, arbitrary native command or raw
secret getter. The [security boundary](../SECURITY.md) remains unchanged:
authorized recipients and independent browser tools may still observe values.

## Validate or publish a candidate

```bash
make test-sdk
make test-sdk-native
# With TypeScript 5.9+ available on PATH:
make test-sdk-types
```

The native SDK test runs real CLI, authenticated IPC, HTTP and process delivery,
with synthetic custody/consent and a synthetic CDP peer, including one-time input. It checks actual canary
delivery and absence of canaries in SDK results. It does not qualify macOS native
dialogs, installed services, browser UI or signed releases.

Public distribution still needs an owned npm scope, release version, signing and
native qualification, then native-package publication followed by the matching
launcher/SDK. See [distribution](distribution.md) and [release gates](qualification/release.md).
The SDK is already part of the package allowlist and follows npm's
[entry-point/package metadata](https://docs.npmjs.com/cli/v11/configuring-npm/package-json/).
