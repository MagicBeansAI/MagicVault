# Installed extension and native-host acceptance

Status: manual gate. Passing real CDP tests and a synthetic native peer does not
qualify Chrome's actual native-messaging dispatch, site permissions or human UI.

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
3. Open its setup page, copy the exact ID, and install the trusted host:

   ```sh
   "$magicvault_qa_bin" --root "$vault_qa_root" --profile qa extension install \
     --extension-id REPLACE_WITH_EXTENSION_ID \
     --host-executable /absolute/trusted/bin/magicvault-native-host
   ```

   The installer must refuse foreign/existing definitions. The manifest allowlists
   exactly the configured ID. Tokens must not appear in manifests or the wrapper.
4. Start the [local fixture site](browser.md), open its `/login` page, and configure
   the credential's password field for the exact printed origin in the daemon.
   Keep the extension site grant absent initially to exercise refusal.
5. Click Connect and inspect/approve the real native connection dialog. Refresh
   setup status, then discover the browser through CLI/MCP using profile `qa` and
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
| X-07 reconnect | Disconnect/restart worker, host or daemon | Explicit reconnect and fresh handles; no replay of pending/uncertain effects |
| X-08 channel boundary | Inspect CLI/MCP result and sanitized audit | No values, page dumps, raw exceptions or full query URLs in own agent channels |
| X-09 removal | Disconnect, unload extension, then remove exact managed host | Only managed definitions removed; foreign/modified files refused; vault/key/pairing untouched |

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
origin, document or consent checks. Record the gate as NOT RUN until a person
actually performs these steps; the [results](results-2026-09-07.md) are explicit.
