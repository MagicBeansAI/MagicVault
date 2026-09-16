import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { MagicVault, MagicVaultError, createOperationId } from '../../npm/sdk.cjs';

const id = 'aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa';
const fill = () => ({ operation_id: createOperationId(), browser_handle: id, target_handle: id,
  fields: [{ css: '#password', credential_ref: `cred_${id}`, credential_field: 'password' }] });
function fixture(t, profile = 'ok', timeoutMs = 2000) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mv-sdk-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const executable = path.join(root, 'native fixture.cjs');
  fs.writeFileSync(executable, `#!${process.execPath}
const fs = require('node:fs'), path = require('node:path');
const args = process.argv.slice(2), root = args[3], mode = args[1], command = args[4];
const value = flag => args[args.indexOf(flag) + 1];
const log = path.join(root, 'calls');
fs.appendFileSync(log, JSON.stringify(args) + '\\n');
if (mode === 'hang') { setTimeout(() => {}, 10000); return; }
if (mode === 'oversized') { process.stdout.write('CANARY'.repeat(100000)); return; }
if (mode === 'bad-error') { console.error('CANARY private path'); process.exitCode = 1; return; }
if (mode === 'denied-error') { console.error(JSON.stringify({error:'denied'})); process.exitCode = 1; return; }
if (mode === 'malformed') { console.log('CANARY'); return; }
let kind, data;
if (command === 'list-credentials') {
  kind = 'credentials'; data = [{credential_ref:'cred_${id}', label:'Demo account', field_names:['password']}];
} else if (command === 'browser-targets') {
  kind = 'browser_targets'; data = [];
} else {
  kind = command.includes('fill') ? 'fill' : 'delivery';
  let operation_id = value('--operation-id');
  if (['secure-fill', 'secure-prompt-fill'].includes(command)) {
    const file = value('--request-file');
    const request = JSON.parse(fs.readFileSync(file, 'utf8'));
    operation_id = request.operation_id;
    fs.writeFileSync(path.join(root, 'request-evidence'), JSON.stringify({file, request,
      fileMode:fs.statSync(file).mode & 511, directoryMode:fs.statSync(path.dirname(file)).mode & 511}));
  }
  if (mode === 'wrong-id') operation_id = '${id}';
  const pending = mode === 'pending' || (mode === 'settle' && fs.readFileSync(log,'utf8').trim().split('\\n').length < 3);
  const state = pending ? 'pending' : mode === 'denied' ? 'denied' : kind === 'fill' ? 'filled' : 'completed';
  data = kind === 'fill' ? {operation_id,state,fields:['filled'],error:null}
    : {operation_id,kind:command === 'secure-new-process' ? 'process':'http',state,may_have_run:true,error:null};
}
if (mode === 'extra-field') data = {...data, secret:'CANARY'};
console.log(JSON.stringify({kind,data}));
`, { mode: 0o700 });
  return { root, vault: new MagicVault({ root, executable, profile, timeoutMs }),
    calls: () => fs.existsSync(path.join(root, 'calls')) ? fs.readFileSync(path.join(root, 'calls'), 'utf8').trim().split('\n').map(JSON.parse) : [] };
}
test('ESM and CommonJS exports expose the same typed client without starting services', async () => {
  const { createRequire } = await import('node:module');
  const cjs = createRequire(import.meta.url)('../../npm/sdk.cjs');
  assert.equal(cjs.MagicVault, MagicVault);
  assert.match(createOperationId(), /^[0-9a-f-]{36}$/);
  assert.deepEqual(Object.keys(cjs).sort(), ['MagicVault', 'MagicVaultError', 'createOperationId']);
});
test('metadata is unwrapped, fill requests are private, immutable snapshots and cleaned up', async t => {
  const { vault, root } = fixture(t);
  assert.equal((await vault.listCredentials())[0].label, 'Demo account');
  const request = fill(), originalId = request.operation_id;
  const pending = vault.secureFill(request);
  request.fields[0].css = 'MUTATED'; request.operation_id = id;
  const receipt = await pending;
  assert.equal(receipt.operation_id, originalId);
  const evidence = JSON.parse(fs.readFileSync(path.join(root, 'request-evidence')));
  assert.equal(evidence.fileMode, 0o600); assert.equal(evidence.directoryMode, 0o700);
  assert.equal(evidence.request.fields[0].css, '#password');
  assert.equal(fs.existsSync(path.dirname(evidence.file)), false);
});
test('unknown input fields, value-bearing requests, flags and invalid IDs fail before dispatch', async t => {
  const { vault, calls } = fixture(t);
  for (const request of [{...fill(), password:'CANARY'}, {...fill(), operation_id:'--help'},
    {...fill(), fields:[{...fill().fields[0], value:'CANARY'}]}, {...fill(), fields:[]},
    {...fill(), fields:[fill().fields[0], fill().fields[0]]}]) {
    await assert.rejects(vault.secureFill(request), {code:'invalid_request'});
  }
  await assert.rejects(vault.secureNewHttp({operation_id:id, profile_id:id, url:'https://example.com'}), {code:'invalid_request'});
  assert.throws(() => new MagicVault({executable:'relative'}), {code:'invalid_request'});
  assert.throws(() => new MagicVault({profile:'--help'}), {code:'invalid_request'});
  assert.equal(calls().length, 0);
});
test('discovery passes literal arguments, including shell syntax, without shell evaluation', async t => {
  const { vault, calls } = fixture(t);
  const tab = 'tab; $(touch SHOULD_NOT_EXIST)';
  await vault.browserTargets({browser_handle:id, top_origin:'https://example.com', tab_id:tab});
  assert.deepEqual(calls()[0].slice(4), ['browser-targets','--browser-handle',id,'--top-origin','https://example.com','--tab-id',tab]);
});
for (const profile of ['malformed', 'bad-error', 'extra-field', 'oversized', 'wrong-id', 'hang']) {
  test(`closed errors and no replay after ${profile}`, async t => {
    const { vault, calls } = fixture(t, profile, profile === 'hang' ? 1000 : 2000);
    const operation_id = createOperationId();
    await assert.rejects(vault.secureNewHttp({operation_id, profile_id:id}), error => {
      assert(error instanceof MagicVaultError);
      assert.equal(error.code, 'transport_uncertain'); assert.equal(error.operationId, operation_id);
      assert(!String(error.stack).includes('CANARY'));
      assert.equal(error.cause, undefined); assert.equal(error.stdout, undefined); assert.equal(error.stderr, undefined);
      return true;
    });
    assert.equal(calls().length, 1);
  });
}
test('recognized native failures remain closed and cleanup also runs on rejection', async t => {
  const { vault, calls } = fixture(t, 'denied-error');
  await assert.rejects(vault.secureFill(fill()), {code:'denied'});
  const args = calls()[0];
  assert.equal(fs.existsSync(path.dirname(args[args.indexOf('--request-file') + 1])), false);
});
test('polling repeats status reads only and returns denied as a terminal receipt', async t => {
  const { vault, calls } = fixture(t, 'settle');
  const operation_id = createOperationId();
  await vault.secureNewHttp({operation_id, profile_id:id});
  assert.equal((await vault.waitForDelivery(operation_id, {intervalMs:50})).state, 'completed');
  assert.deepEqual(calls().map(c => c[4]), ['secure-new-http','delivery-status','delivery-status']);
  const denied = fixture(t, 'denied');
  assert.equal((await denied.vault.waitForFill(operation_id)).state, 'denied');
  assert.equal(denied.calls().length, 1);
});
test('polling deadline and abort do not cancel or replay the daemon operation', async t => {
  const { vault, calls } = fixture(t, 'pending');
  const operation_id = createOperationId();
  await assert.rejects(vault.waitForDelivery(operation_id, {timeoutMs:160, intervalMs:100}), {code:'wait_timeout', operationId:operation_id});
  assert(calls().every(c => c[4] === 'delivery-status'));
  const abort = new AbortController(); abort.abort('CANARY');
  const before = calls().length;
  await assert.rejects(vault.secureNewHttp({operation_id,profile_id:id}, {signal:abort.signal}), {code:'cancelled'});
  assert.equal(calls().length, before);
});
test('an in-flight abort preserves the operation ID and uncertainty', async t => {
  const { vault, calls } = fixture(t, 'hang');
  const abort = new AbortController(), operation_id = createOperationId();
  const pending = vault.secureNewHttp({operation_id,profile_id:id}, {signal:abort.signal});
  const timer = setTimeout(() => abort.abort('CANARY'), 1000);
  t.after(() => clearTimeout(timer));
  await assert.rejects(pending, {code:'transport_uncertain', operationId:operation_id});
  assert.equal(calls().length, 1);
});

test('one-time prompt fill takes metadata only, snapshots it and cleans its private file', async t => {
  const {vault,root,calls} = fixture(t);
  const request = {operation_id:createOperationId(), browser_handle:id, target_handle:id,
    fields:[{css:'#password',field_name:'password'}]};
  const pending = vault.securePromptFill(request);
  request.fields[0].field_name = 'changed';
  const receipt = await pending;
  assert.equal(receipt.state, 'filled');
  assert.equal(calls()[0][4], 'secure-prompt-fill');
  const evidence = JSON.parse(fs.readFileSync(path.join(root, 'request-evidence')));
  assert.equal(evidence.request.fields[0].field_name, 'password');
  assert.equal(evidence.fileMode, 0o600);
  assert.equal(evidence.directoryMode, 0o700);
  assert.equal(fs.existsSync(path.dirname(evidence.file)), false);
  const before = calls().length;
  for (const bad of [
    {...request,value:'CANARY'}, {...request,save:true}, {...request,approved:true},
    {...request,fields:[{css:'#password',field_name:'password',value:'CANARY'}]},
    {...request,fields:[{css:'#password',field_name:'password',credential_ref:'cred_'+id}]},
    {...request,fields:[{css:'#a',field_name:'password'},{css:'#b',field_name:'password'}]},
    {...request,browser_handle:'00000000-0000-0000-0000-000000000000'},
  ]) await assert.rejects(vault.securePromptFill(bad), {code:'invalid_request'});
  assert.equal(calls().length, before);
});
