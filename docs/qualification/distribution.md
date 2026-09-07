# Native distribution acceptance

This is a **manual acceptance runbook**, not evidence that it has been run.
Automated package tests exercise real native executables/IPC with synthetic
custody and application-only installation. They cannot establish real keychain,
LaunchAgent, OS consent, installed-extension or publisher verification behavior.

## Preconditions

- Use a disposable macOS Apple Silicon **OS account**, with explicit permission
  to create its keychain item, LaunchAgent and browser native-host definitions.
  A disposable Chrome profile alone does not isolate these OS-user resources.
- Obtain matching reviewed CLI/MCP/native-host/extension candidate packages.
  Record package version, commit, binary/package hashes, OS, Node and browser.
  Do not copy a personal vault, pairing capability or credential into the account.
- Use only synthetic values and the repository's local browser/process/HTTP
  fixtures. No real logins, external form submission or paid/destructive actions.
- For a signed-release gate, verify the Developer ID signer and notarization of
  the exact downloaded executables. Record actual Gatekeeper results on a fresh,
  quarantined download. Never disable Gatekeeper or remove quarantine merely to
  turn a failure into a pass. Unsigned candidates cannot pass this signing gate.

## Fresh setup, denial and restart

1. Install both local tarballs with npm as described in the public quick start.
   Confirm no `~/.magicvault`, `~/.magicvault-app`, LaunchAgent or keychain item
   was created by npm. Run `--version` and `doctor`; both must be credential-free.
2. Run `magicvault --profile agent setup`. Check the separate private app/vault
   directories, per-instance LaunchAgent, daemon readiness and native pairing
   dialog. First deny pairing: the command must fail without a saved capability.
   Rerun setup and approve; confirm it reuses the same instance/key identity.
3. Copy the printed `mcpServers` configuration into a disposable MCP client.
   Confirm the absolute path is under the stable app directory, not npm's cache,
   a build target or a temporary directory. No token belongs in that config.
4. Enroll a synthetic credential in the hidden prompt. Confirm discovery returns
   references/field names, never the value. Deny a use and confirm no delivery.
5. Log out/in, or explicitly stop/start the owned service. Confirm readiness and
   existing credentials/policies; no replacement key or duplicate pairing should
   be created. Record additional OS permissions/keychain prompts accurately.

## Browser and new destinations

Load the printed stable extension directory unpacked in Chrome. Normal setup
already registers its fixed native identity; source/custom builds use the
documented explicit installer. Grant only the synthetic fixture origin and
approve the first automatic connection after matching its profile ID. No manual
ID copying or Connect step is needed on the normal packaged path.
Follow the [extension acceptance](extension.md). Verify the host path is
under the stable app directory and no secret appears in tool responses.
Separately follow [CDP](browser.md) and [process/HTTP](process.md) runbooks. npm
packaging must not change recipient binding, per-use consent or receipt-only output.

## Upgrade, cache removal and interrupted setup

1. Install a second reviewed candidate's matching npm packages. Confirm that this
   alone does not restart or replace the running daemon. Run explicit `upgrade`.
2. While a synthetic operation or native prompt is pending, confirm unload/drain
   happens before activation. A blocked drain must refuse activation. Cancellation
   or a lost reply is not rollback, and never triggers an automatic replay.
3. Reconnect MCP and reload/reconnect the extension. Verify retained instance/key,
   credentials, permissions and delivery profiles. Old browser targets and jobs
   must not silently regain authority.
   Verify the browser uses current assets, not a resolved old version directory;
   remove/load unpacked again when needed and check/rebind the exact extension ID.
4. Remove **only these test packages** with npm. Confirm the stable service,
   native host, extension assets and direct MCP command remain usable.
5. Test a changed owned definition and a foreground daemon holding the writer
   lease. Setup/upgrade must fail closed instead of taking over or force-killing.
   Test a truncated synthetic bundle: current must remain on its complete version.
6. In a disposable account only, interrupt setup/upgrade at staged states. Record
   whether the old/new version is selected, whether the daemon is loaded/ready,
   and which private artifacts remain. Do not count ambiguous recovery as success
   or delete unknown files to hide the failure.

## Recoverable removal

Run the installed native `magicvault uninstall`. Confirm the exact managed service
is unloaded and definitions removed, app files have moved to the reported archive,
and the separate vault/keychain/pairing state remains. Remove the test extension
and MCP configuration manually. Check that unrelated services/files remain intact.
Uninstall is not provider credential revocation or secure erasure.

Retain a value-free result using the [evidence format](README.md#evidence-format).
Never commit capabilities, keychain exports, private signing logs, browser profiles
or raw prompt/transport captures. Report blocked cases as blocked, not passed.
