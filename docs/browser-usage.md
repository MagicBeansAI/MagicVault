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
   Do not load the raw `extension/` directory: it intentionally lacks the shared
   `fill.js` copied by packaging, and its service worker cannot start on its own.
2. In a disposable Chrome/Chromium profile's extension management page, enable
   developer mode and load the unpacked `dist/extension` directory. Store
   distribution/signing is not part of this source checkpoint.
3. Normal packaged `magicvault setup` already installs the native host. The
   extension's public, fixed ID is shown in a selectable chip; it does not need
   to be copied. For source builds, install the host once at a stable absolute path:

   ```sh
   magicvault --profile agent extension install \
     --host-executable /absolute/trusted/bin/magicvault-native-host
   ```

4. In the setup page, choose **Allow all HTTPS websites** for one-time access,
   or **Allow site** for each selected website. Browser permissions cover paths
   and ports; the daemon separately enforces the exact origins configured above.
   Both top-page and frame sites need access for embedded logins. No browser
   permission is granted automatically, including when upgrading the extension.
   Local HTTP (`localhost` / `127.0.0.1`) always needs its own explicit site grant.
5. Start the daemon. The extension automatically connects: match the **Browser
   profile ID** shown on its setup page in the first native dialog, then approve.
   Setup status updates automatically. Discover the extension's
   browser and targets through the same CLI/MCP commands used for CDP.

### Access modes and disabled sites

**All HTTPS websites** removes repeated browser site prompts after human approval.
The **Current browser access** banner shows **All HTTPS websites enabled** and
the enable button becomes disabled with that same label. This is read back from
Chrome when setup opens, regains focus or permissions change, not inferred from
a previous click. Selected-site, no-access and unverified states are distinct;
blocklist-loading errors are shown separately from browser permission status.
It does not change credential permissions, per-fill consent or Chrome's own site
restrictions. **Clear HTTPS access — use selected sites** removes all HTTPS
grants; then add individual sites with **Allow site**. This reset retains local
HTTP grants and the blocklist; it does not reconstruct earlier HTTPS grants.
Broad access increases the reach of a compromised extension; use selected sites
when browser-enforced least privilege matters.

Enter an origin and choose **Disable site in MagicVault** to exclude that site
from discovery and fills in either access mode. Re-enable an entry using the
disabled-sites list. Entries match the exact scheme and hostname on every port
and path (including the trailing-dot DNS spelling); subdomains need separate
entries. A blocked top page excludes all its frames; a blocked frame is also
refused on an otherwise allowed top page. Allowing a site does not remove a block.

Blocks are rechecked before dispatch, including for previously discovered handles.
A change during preflight causes refusal; already dispatched fills cannot be
recalled. A stale result is not permission to retry automatically. The blocklist
is a MagicVault behavior rule, not subtraction from a broad Chrome permission or
protection against compromised extension code. **Remove site permission** cannot
disable just one HTTPS host while all-HTTPS permission remains granted; use the
blocklist or reset to selected-site grants instead.

At most 256 canonical site entries are stored in trusted-context-only
`chrome.storage.local`, never in browser sync. No credentials, page contents or
fill payloads are stored. Worker/browser restarts retain blocks; removing the
extension clears them. Missing policy on first use means no blocks, with browser
grants still required; unreadable or malformed policy refuses operations. A
failed settings write is reported, not presented as a successful change. Use a
fresh discovery after changes, and deliberately restore blocks if reinstalling.

### Native connection and removal

The macOS installer creates exact allowed-origin native host definitions for
Chrome and Chromium under their user `NativeMessagingHosts` directories, plus
a private instance-owned launch wrapper and configuration. Setup/install is
idempotent and repairs exact managed files, including an interrupted identity
migration. Modified/foreign definitions, another root/client/executable and
symlinks are refused; it never replaces executables. No token is written to the
manifest or wrapper: the host loads the existing private client pairing.
Only one configured host installation is supported per user for this host name.

Each browser profile generates a random ID and 256-bit pairing capability in
trusted-context-only local storage (not browser sync). This capability authorizes
reconnection/target discovery, **not credential use**; it is never returned to
CLI/MCP, website code or the options page. The daemon stores only its hash, scoped
to the client, extension and profile, with the human's remembered decision.
Connection labels include the profile ID. Multiple profiles coexist; a live
duplicate/copy of a profile is refused rather than replacing its connection.
Per-fill native consent includes the browser handle shown in the setup status,
so two profiles visiting the same origin can still be distinguished.

Approved profiles reconnect after worker/browser/host/daemon restart. Transient
transport failures and a busy daemon use Chrome alarms with approximately
30/60/120/240/300-second delays, up to five seconds of jitter; Chrome can delay
alarms further. Worker wakes cannot bypass the saved deadline. Denial, revoked
pairing, duplicate identity, capacity, incompatible wire and approval timeout
stop retries. **Retry / resume** explicitly retries; reapproval still uses native
human consent. **Pause connection** closes this profile's channel immediately and
persists across restarts without forgetting an existing approval.

`magicvault disconnect-browser --browser-handle HANDLE` revokes that extension
profile's remembered authorization and cancels its pending effects. Other profiles
are unaffected. To revoke an offline/lost profile, revoke its client pairing
(`magicvault revoke-client --client-id ID`); this revokes all that client's profiles.
Up to 128 remembered profile decisions and the existing active-browser limit are
supported. Reinstalled profiles consume new decisions; client revocation clears
them. Clearing extension storage is **not** a daemon-side revocation.

Reconnect always creates new browser/target handles; no pending or uncertain
effect is replayed. Closing the connection never closes the browser. The extension
does not auto-start a stopped daemon; normal managed setup starts its LaunchAgent.

### Checking extension readiness

After normal packaged setup, no ID copy or manual Connect is needed. Initial
website access, the first native approval for each browser profile and per-fill
consent remain deliberate human steps. Starting `serve` alone does not install a
native host; source builds still need the host-install command above. Paused or
terminally refused connections require explicit Retry / resume.

Normal `setup` returns an advisory `extension` snapshot. Check again at any time
using the same paired CLI/MCP profile as the native host:

```bash
magicvault --profile agent doctor
```

| Output | Meaning | Next step |
| --- | --- | --- |
| `native_host.state: verified` | This vault's exact host definitions, executable safety and local pairing structure passed read-only checks | This alone does not prove Chrome loaded the extension or the daemon accepts the pairing |
| `native_host.state: not_configured` or `incomplete` | This vault has no config, or expected executable/definition files are missing | Run normal setup, or the explicit source/custom host installer; never delete the vault to repair it |
| `native_host.state: unavailable` | Invalid/foreign/unsafe files, unreadable state or unsupported platform | Follow the closed error and inspect owned definitions; no automatic replacement |
| `connection.state: connected` | At least one live extension profile is visible to this paired client | Presence is confirmed; website access and credential policy/consent still apply |
| `connection.state: not_connected` | The daemon answered with no extension connections for this client (CDP does not count) | Open the browser and inspect extension setup for installation, enabled/Pause, approval and retry status |
| `connection.state: unavailable` | The paired metadata query failed or exceeded two seconds | Check daemon/pairing health; rerun using `native_host.client_profile` if different |

`browser_installation` is `confirmed_connected` only with a live extension
connection; otherwise it is `unconfirmed`, **never inferred absent**. A closed
browser, disabled/uninstalled/paused extension, pending consent, transient retry,
or a different paired client can all prevent confirmation. Retained approvals
and packaged assets are not proof of installation. The snapshot does not enumerate
all Chrome profiles or inspect their files, open a browser, resume a paused
connection, prompt for approval or grant website access. It is advisory, not an
ongoing monitor or a gate on CDP/process/HTTP use. Retry alarms can take several
minutes; setup need not wait for a first connection. See Chrome's
[extension-initiated native messaging model](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging).

### Upgrading older unpacked extensions

Build/install matching 0.6.x standalone binaries and extension 0.5.x. Native
handshake v2 has no silent v1 fallback. Run `magicvault setup` (or the source host
install command above) to select the bundled ID, then reload the packaged assets.
The one-time move from a path-derived ID may require removing the old extension
and loading the new one, then deliberately restoring site grants/blocks. The new
fixed ID is `ljbccephkkklnlloibgcgcopcffbbcdc`; subsequent directory changes retain
that identity. Check the setup chip before approving. Pause/unload any old copy.

The manifest's public key is an **unpacked-build identity**, not publisher signing
or a Chrome Web Store listing; no private signing key is included or retained.
A future Store identity can require a separately documented migration. Custom
builders may still use `--extension-id ID`; `upgrade` preserves that selected ID,
while explicit `setup` selects the bundled ID. `setup --install-only` does not
register a host, initialize a vault or start a service.

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
Site access uses [optional Chrome permissions](https://developer.chrome.com/docs/extensions/reference/api/permissions)
and the blocklist uses [trusted-context local storage](https://developer.chrome.com/docs/extensions/reference/api/storage).
These API references explain the implementation choices; they are not evidence
that this repository's end-to-end workflows have passed qualification.
