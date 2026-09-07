# Installed extension and native-host acceptance

Status: manual gate for genuine permissions, native consent/keychain and normal
installation lifecycle. [Basic installed acceptance](results-discovery-2026-09-08.md)
passed on one macOS/Chrome setup; that is not completion of the matrix below.
[Automated native transport](extension-transport.md) now
exercises actual Chrome dispatch and multiple profiles with synthetic custody/UI
and a pregranted loopback fixture. That does not qualify these native UX gates.

## Isolation and setup

Use a disposable OS account if possible. Chrome native-host definitions live at
OS-user scope and apply across that user's browser profiles. A new Chrome profile
does **not** isolate those definitions or the keychain. Never overwrite an
existing host registration to make a test pass.

1. Follow [CLI native setup](cli.md) with a new root and profile `qa`. Keep the
   absolute root, executable paths and generated extension ID local, not in Git.
2. Run `make package-extension`, then open `chrome://extensions` in a fresh
   browser profile, enable Developer mode and load `dist/extension` unpacked.
   It must contain `manifest.json`, `worker.js`, `fill.js` and the options assets.
3. Normal packaged setup registers the fixed bundled ID automatically. For source
   builds install the trusted host (no ID copying required):

   ```sh
   "$magicvault_qa_bin" --root "$vault_qa_root" --profile qa extension install \
     --host-executable /absolute/trusted/bin/magicvault-native-host
   ```

   The installer must refuse foreign/modified definitions and repair only exact
   managed files for this root/client/executable. The manifest allowlists
   exactly the configured ID. Tokens must not appear in manifests or the wrapper.
4. Start the [local fixture site](browser.md), open its `/login` page, and configure
   the credential's password field for the exact printed origin in the daemon.
   Keep the extension site grant absent initially to exercise refusal.
5. The extension connects automatically. Match its displayed browser profile ID
   in the first native dialog and approve. Watch setup status update, then discover
   the browser through CLI/MCP using profile `qa` and
   the same root. Do not pass native-host configuration or pairing tokens to a model.

## Acceptance cases

| Case | Action | Required observation |
| --- | --- | --- |
| X-01 identity | Use correct extension ID, then a separate deliberate wrong-ID fixture | Correct host connects only after consent; wrong ID never receives material |
| X-02 no site grant | Discover/request fill before granting the fixture site | Target omitted/refused; no value enters the page |
| X-03 explicit grant | Grant the loopback site, rediscover, request/approve fill | Only the bound field receives the synthetic value; status is value-free |
| X-04 permission removal | Remove site permission and attempt a freshly bound fill | Refused; no implicit regrant or fallback to another browser transport |
| X-05 controls/frames | Use `/controls` and `/frames` case matrix | Strict refusal/partial semantics; correct same-origin frame; opaque target never receives a value |
| X-06 consent race | Navigate/close tab while fill consent is pending | Replacement/closed target is not filled |
| X-07 reconnect | Restart worker, host or daemon after approval | Automatic reconnect with backoff and fresh handles; no repeated connection approval or replay of pending/uncertain effects; use consent remains separate |
| X-08 channel boundary | Inspect CLI/MCP result and sanitized audit | No values, page dumps, raw exceptions or full query URLs in own agent channels |
| X-09 removal | Disconnect, unload extension, then remove exact managed host | Only managed definitions removed; foreign/modified files refused; vault/key/pairing untouched |
| X-10 all HTTPS | In a fresh profile, deny then approve **Allow all HTTPS websites**; use synthetic data and a trusted HTTPS test origin | Denial preserves grants; approval discovers HTTPS without per-site grants; local HTTP still requires its own grant; credential policy and native per-use or exact remembered consent still apply |
| X-11 selected reset | Grant all HTTPS and local HTTP, then **Clear HTTPS access — use selected sites** | HTTPS grants cleared, local HTTP and blocks retained; individual HTTPS grants work afterward |
| X-12 site blocks | Disable allowed top/frame origins, including after discovery but before fill approval; repeat with alternate ports/trailing-dot host | Blocked targets omitted/refused even under all-HTTPS grant; a blocked top excludes all frames; unrelated sites remain usable |
| X-13 restart/settings | Reopen setup in two tabs, block/unblock, restart worker/browser; allow a blocked site | Stored blocks remain; setup tabs refresh; allowing a site does not clear its block; re-enable removes only the block, not credential checks |
| X-14 live revocation | Remove Chrome permission or disable site during preflight; separately change after dispatch | No new dispatch after observed revocation; already dispatched work is not claimed recalled; no automatic retry |
| X-15 reload | Package updated fixed-ID sources and reload | Worker registers with actual `fill.js`; setup renders; fixed ID/grants/blocks retained; reconnect automatically; no automatic all-HTTPS request. Legacy path-derived IDs follow the documented one-time migration |
| X-16 profiles | Load the same extension in two independent profiles; approve each once; restart one | Both appear with distinct profile labels/handles, retain separate connections, and one never evicts the other |
| X-17 copied profile | In a disposable fixture only, duplicate a profile identity while its original is connected | Duplicate is refused with stopped retries; original remains usable; no replacement loop |
| X-18 pause/denial | Pause and restart browser; resume; deny first authorization; restart again | Pause persists; denial does not reprompt automatically; Retry explicitly requests new native consent |
| X-19 revocation | Disconnect one native browser through CLI, then separately revoke its client | Profile cannot auto-reconnect; unrelated profiles survive profile revocation; all client profiles stop after client revocation |
| X-20 outage | Leave daemon unavailable through several alarms; reopen options/restart worker | 30–300-second backoff (up to five seconds jitter, Chrome may delay); no overlapping hosts, prompt floods or operation replay |
| X-21 large profile | With many unrelated or granted tabs, discover using exact `top_origin` and optionally backend `tab_id` | Narrowing happens before inspection bounds; only matching permitted targets returned; exact ports and actual documents rechecked. Oversized matching sets still return `capacity`, not a partial list; no grant changes or unrelated tab closure |
| X-22 consent modes | Allow once, deny the next new use; then Always allow and request a fresh matching fill | Allow once does not persist; Always allow skips only exact matching use prompts; fresh target/document validation still runs |
| X-23 remembered permission removal | With an exact-use grant, remove fixture site access or block the site, then attempt a fresh fill | No delivery despite remembered use; no regrant/fallback; restoring site access does not itself delete the remembered use grant |
| X-24 remembered scope/recovery | Change selector/frame mapping or use another profile; reconnect the approved profile; finally revoke the use grant | Changed scope asks separately; exact extension-profile grant survives reconnect; revoked scope asks again; no pending operation replay |

For X-09 use the same explicit root:

```sh
"$magicvault_qa_bin" --root "$vault_qa_root" --profile qa extension remove
```

Disconnect **before** removing definitions; removal does not stop an existing
host connection. The production installer initially supports one configured host
installation per OS user. Keep partial-install evidence for deliberate recovery
instead of deleting a vault or silently replacing another application's files.

If a case fails, record the closed error and exact step. Check daemon readiness,
profile/root match, absolute executable location, extension ID, site permission,
native manifest conflicts and unpacked assets. Never “fix” it by weakening UID,
origin, document or consent checks. Record each case as NOT RUN until a person
actually performs its steps. The [current installed results](results-discovery-2026-09-08.md)
distinguish basic acceptance from the unexecuted cases.
