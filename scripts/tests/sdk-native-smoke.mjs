// Launched only by the Rust integration fixture: real CLI/IPC/effects, synthetic custody/consent/CDP.
import assert from 'node:assert/strict';
const { MagicVault, MagicVaultError, createOperationId } = await import(
  process.env.MAGICVAULT_TEST_NODE_SDK || new URL('../../npm/sdk.cjs', import.meta.url).href);

const vault = new MagicVault({ executable: process.argv[2], root: process.argv[3] });
const visible = [];
const capture = value => { visible.push(value); return value; };
assert.equal(capture(await vault.status()).ready, true);
const credentials = capture(await vault.listCredentials());
assert.equal(credentials.length, 1);
const browsers = capture(await vault.listBrowsers());
assert.equal(browsers.length, 1);
const targets = capture(await vault.browserTargets({browser_handle:browsers[0].browser_handle, top_origin:'https://example.com'}));
assert.equal(targets.length, 1);
const operation_id = createOperationId();
capture(await vault.secureFill({operation_id, browser_handle:browsers[0].browser_handle,
  target_handle:targets[0].target_handle,
  fields:[{css:'#password', credential_ref:credentials[0].credential_ref, credential_field:'token'}]}));
assert.equal(capture(await vault.waitForFill(operation_id, {timeoutMs:10000})).state, 'filled');
const fresh = capture(await vault.browserTargets({browser_handle:browsers[0].browser_handle, top_origin:'https://example.com'}));
const jit = {operation_id:createOperationId(), browser_handle:browsers[0].browser_handle,
  target_handle:fresh[0].target_handle, fields:[{css:'#password',field_name:'password'}]};
capture(await vault.securePromptFill(jit));
assert.equal(capture(await vault.waitForFill(jit.operation_id, {timeoutMs:10000})).state, 'filled');
assert.equal((await vault.listCredentials()).length, credentials.length);
const profiles = capture(await vault.listDeliveryProfiles());
assert.equal(profiles.length, 2);
for (const profile of profiles) {
  const request = {operation_id:createOperationId(), profile_id:profile.profile_id};
  capture(await (profile.kind === 'http' ? vault.secureNewHttp(request) : vault.secureNewProcess(request)));
  const receipt = capture(await vault.waitForDelivery(request.operation_id, {timeoutMs:10000}));
  assert.equal(receipt.state, 'completed');
  assert.equal(receipt.may_have_run, true);
  assert.equal(receipt.kind, profile.kind);
}
await assert.rejects(vault.deliveryStatus(createOperationId()), error => error instanceof MagicVaultError && error.code === 'not_found');
const wire = JSON.stringify(visible);
assert(!wire.includes('CANARY'));
assert(!wire.includes('SYNTHETIC-URL'));
assert(!wire.includes('stdout'));
assert(!wire.includes('stderr'));
console.log('SDK native smoke: discovery, saved and one-time browser fills, HTTP, process and missing-status checks passed; receipts contain no canaries.');
