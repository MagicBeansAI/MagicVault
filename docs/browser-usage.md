# Browser credential fills

Browser-delivery alpha. Automated tests and disposable headed/headless Chrome
and CLI-to-Chrome cases [pass](qualification/results-2026-09-07.md) on the recorded
configuration. The installed-extension/native-human/keychain workflow remains a
[manual acceptance gate](qualification/extension.md). Begin with synthetic
credentials and disposable profiles; see [qualification](testing.md).

## Prepare custody and browser permission

Complete [daemon setup, pairing and enrollment](setup.md) first. Use the same
paired profile for browser registration, policy, discovery and agent operations.
If an owner enrolled the credential in another profile, obtain metadata consent
for the agent profile first. Neither pairing nor metadata consent grants delivery.

```sh
magicvault --profile agent configure-browser-credential \
  --credential-ref cred_REPLACE_WITH_ENROLLED_UUID \
  --field username --field password \
  --origin https://accounts.example.com
```

The daemon asks the human to authorize this client's selected fields and exact
origins. Both the top-level page and selected frame must have an allowed origin.
For an embedded login, explicitly allow the top page and the login frame origin;
this lets both origins receive the selected fields, so only include trusted ones.
There are no wildcard domains or implicit HTTP downgrades. HTTPS is required
except for explicitly selected loopback development sites. Scheme and port matter.
The policy accepts at most 16 origins and eight field names per credential/client.

To remove that client's browser permission, repeat the command with the selected
fields and **no** `--origin`. Configuration requires native consent and cancels
outstanding jobs for that client; it cannot recall values already delivered.
The existing core metadata policy is not rewritten: these explicit standalone
rules authorize the new browser effect independently of metadata discovery.

## Direct CDP

Use a trusted Chrome/Chromium automation profile exposing an accessible debugging
websocket. MagicVault connects alongside the original automation tool; it does
not proxy, launch, navigate, submit or close the browser. A driver using only a
private debugging pipe needs explicit driver setup/integration first.

Configure the browser with a separate `--user-data-dir` and a loopback debugging
port. For example, a human can launch a disposable macOS Chrome profile:

```sh
browser_profile=$(mktemp -d)
'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  --user-data-dir="$browser_profile" \
  --remote-debugging-address=127.0.0.1 --remote-debugging-port=9222
```

Add `--headless=new` for modern headless Chrome; native approval still requires
the daemon's interactive desktop session. Do not reuse a
personal/default profile or expose the port to the network. Manage and remove
only the disposable profile you created, after closing that browser.

Obtain the browser websocket from the trusted launcher's discovery/startup output,
then register it from the human CLI:

```sh
magicvault --profile agent register-cdp --label 'Automation browser' \
  --endpoint ws://127.0.0.1:9222/devtools/browser/REPLACE_WITH_BROWSER_ID
magicvault --profile agent list-browsers
magicvault --profile agent browser-targets --browser-handle REPLACE_WITH_BROWSER_HANDLE
```

The endpoint must be a loopback IP-literal `ws://.../devtools/browser/...` with an
explicit port, no user information, query token or fragment. HTTP discovery URLs,
page websockets, remote hosts and ambient proxy/authentication are not accepted.
Keep the debugging endpoint out of public reports and model messages.

Target discovery returns safe origins, backend tab/frame IDs and short-lived
MagicVault handles—not titles, full URLs, DOM dumps or values. Match the backend
tab/frame explicitly with your existing tool. Do not guess between same-origin
tabs. Discovery replaces unused handles from that browser; it does not revoke
the document binding already captured by a pending fill.

The adapter rechecks frame/document loaders, uses a dedicated isolated world and
a system-unique execution context, and fails closed if the browser cannot supply
that binding. Out-of-process frames that cannot be addressed through the attached
page session are unsupported by this initial CDP adapter; use the extension path
where its document-targeted frame access and site permissions apply.

## Chromium extension

This path needs no debugging endpoint. It uses a narrowly scoped extension, a
native messaging executable, and the same daemon authorization as CDP.

1. Build the executables as described in [setup](setup.md), then run
   `make package-extension` to assemble `dist/extension`. Packaging only copies
   source assets; it neither installs a host nor launches a browser.
2. In a disposable Chrome/Chromium profile's extension management page, enable
   developer mode and load the unpacked `dist/extension` directory. Store
   distribution/signing is not part of this source checkpoint.
3. Open the extension's setup page and copy its **extension ID**. It is not a
   credential. The ID may change if you load from a different directory.
4. Install the native host, using a stable absolute path to the executable:

   ```sh
   magicvault --profile agent extension install \
     --extension-id REPLACE_WITH_EXTENSION_ID \
     --host-executable /absolute/trusted/bin/magicvault-native-host
   ```

5. In the extension setup page, grant the required site permissions. Browser
   permissions cover the host's paths and ports; the daemon separately enforces
   the exact origins configured above. Grant both top-page and frame sites when
   filling an embedded login. No site is granted automatically.
6. Start the daemon, click **Connect**, and approve the daemon's native connection
   dialog. Refresh the setup status after responding. Discover the extension's
   browser and targets through the same CLI/MCP commands used for CDP.

The macOS installer creates exact allowed-origin native host definitions for
Chrome and Chromium under their user `NativeMessagingHosts` directories, plus
a private instance-owned launch wrapper and configuration. It refuses to
overwrite existing definitions or executables. No token is written to the
manifest or wrapper: the host loads the existing private client pairing.
Only one configured host installation is supported per user for this host name.

The extension keeps its native connection while active. After worker/host/daemon
disconnect, explicitly reconnect and obtain new handles; pending effects are not
replayed. An approved reconnect replaces the previous extension connection for
that client. Closing the connection never closes the browser.

For removal, disconnect in the extension and unload/remove it from the browser,
then run `magicvault extension remove` for the same instance root. Removal checks
the exact managed definitions before deleting them. Modified/foreign definitions
are refused. Vaults, pairings and keys remain untouched. Removal alone does not
stop an already connected host: disconnect first. Retain partially installed
files for deliberate inspection instead of deleting a vault to repair setup.

## Request a fill and inspect its outcome

Use `secure_fill` in MCP, or create a JSON file matching the
[reference-only example](../examples/secure-fill.json). Replace the illustrative
IDs with your actual handles and credential reference, and choose a fresh
operation UUID once (for example, using `uuidgen`). Never put values in the file.

```sh
magicvault --profile agent secure-fill --request-file /absolute/path/fill-request.json
magicvault --profile agent fill-status --operation-id REPLACE_WITH_OPERATION_UUID
```

The first reply is normally `pending`: respond to the native dialog, then poll.
That dialog authorizes only these fields, selectors and this destination/document.
Authorization is revalidated immediately before material leaves custody. Target
handles expire after 180 seconds and are consumed once. The target must still
exist, match the original document and origin, and contain exactly one supported
input for each selector at execution time.

| State | Meaning / next action |
| --- | --- |
| `pending` | Await native human decision; do not issue another fill |
| `filling` | Authorized delivery is in progress; poll the same operation |
| `filled` | All requested fields reported delivery; continue with the original browser tool |
| `denied`, `expired`, `cancelled`, `failed` | Inspect the closed error and per-field status; never fall back to requesting raw values |
| `partial` | Some fields were filled; no rollback or login success is claimed |
| `uncertain` | Delivery or its durable receipt may have happened; reconcile before any new attempt |

Per-field statuses preserve request order and contain only `filled`, `not_filled`
or `uncertain`. Filling does not prove authentication succeeded. The adapter does
not explicitly submit the form, but a site's input/change handlers can have their
own side effects; that website is an authorized recipient.

To request cancellation:

```sh
magicvault --profile agent cancel-fill --operation-id REPLACE_WITH_OPERATION_UUID
magicvault --profile agent fill-status --operation-id REPLACE_WITH_OPERATION_UUID
```

Cancellation is best effort after dispatch. A lost response is not permission to
retry under a new ID. Detailed results are retained for ten minutes; spent IDs
remain refused for the daemon epoch. Restart invalidates all browser/target/job
handles. `not_found` after restart/expiry is not proof of failure or safe retry.
Typed audit receipts help a trusted operator reconcile; audit files are not MCP
tools and should not be uploaded as raw diagnostics.

## Supported controls and deliberate limits

Selectors use up to 512 printable ASCII bytes; CSS escapes can address Unicode
identifiers without putting directional/control characters in the human prompt.
The fixed fill function supports visible, enabled, writable HTML text/password/
email/tel/url/search inputs, native setters and input/change events. It validates
all selectors before the first write and detects later detach/replacement.
Hidden inputs, file pickers, browser-internal pages, ambiguous matches, arbitrary
JavaScript, custom controls, cross-shadow-root selectors and opaque origins are
not guessed. Frames require explicit document identity and backend support.

This feature does not filter another tool's later DOM, screenshots, traces,
cookies or session observations. Password masking is not evidence of such
filtering. Read [security](../SECURITY.md) and [builder responsibilities](integrations.md).

## Protocol references

The adapter uses documented [CDP isolated worlds](https://chromedevtools.github.io/devtools-protocol/tot/Page/#method-createIsolatedWorld)
and [system-unique runtime contexts](https://chromedevtools.github.io/devtools-protocol/tot/Runtime/#method-callFunctionOn).
The extension uses [native messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)
and [document-targeted isolated script execution](https://developer.chrome.com/docs/extensions/reference/api/scripting).
These API references explain the implementation choices; they are not evidence
that this repository's end-to-end workflows have passed qualification.
