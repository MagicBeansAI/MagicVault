import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { assemble, binaries, platforms } from '../package-npm.mjs';
import { packRelease, releasePlan, publishPlan } from '../release-npm.mjs';

const repo = path.resolve(import.meta.dirname, '../..');
const scope = '@magicvault-release-fixture';
const publishFixture = (plan, invoke) => publishPlan(plan, invoke, () => {});
function executable(platform) {
  const bytes = Buffer.alloc(256), [system, arch] = platform.split('-');
  if (system === 'darwin') {
    bytes.writeUInt32LE(0xfeedfacf); bytes.writeUInt32LE(arch === 'arm64' ? 0x0100000c : 0x01000007, 4);
  } else if (system === 'linux') {
    Buffer.from([0x7f, 69, 76, 70, 2, 1]).copy(bytes); bytes.writeUInt16LE(arch === 'arm64' ? 183 : 62, 18);
  } else {
    bytes.write('MZ'); bytes.writeUInt32LE(128, 60); bytes.writeUInt32LE(0x00004550, 128);
    bytes.writeUInt16LE(arch === 'arm64' ? 0xaa64 : 0x8664, 132); bytes.writeUInt16LE(0x20b, 152);
  }
  return bytes;
}
test('real offline packs form a complete release; README matches scope and tampering is refused', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mv-release-fixture-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const previous = process.env.npm_config_cache;
  process.env.npm_config_cache = path.join(root, 'cache');
  t.after(() => { if (previous === undefined) delete process.env.npm_config_cache; else process.env.npm_config_cache = previous; });
  let version;
  for (const platform of platforms) {
    const binaryDir = path.join(root, `${platform}-bin`), output = path.join(root, `${platform}-packages`);
    fs.mkdirSync(binaryDir);
    for (const name of binaries) fs.writeFileSync(path.join(binaryDir, name + (platform.startsWith('win32-') ? '.exe' : '')), executable(platform));
    const built = assemble({ repo, binaryDir, output, scope, platform, registryReadme: true });
    version = built.version;
    const readme = fs.readFileSync(path.join(output, 'launcher/README.md'), 'utf8');
    assert(readme.includes(`npm install ${scope}/magicvault@alpha`));
    assert(readme.includes(`from '${scope}/magicvault'`));
    assert.doesNotMatch(readme, /@magicvault-local|not a published npm|\.tgz/);
    for (const osName of ['macOS', 'Linux', 'Windows']) assert(readme.includes(`${osName}-alpha-orange`));
    const entries = packRelease({ packages: output, output: path.join(root, `MagicVault-alpha-${platform}`), scope, platform });
    assert.equal(entries.length, platform === 'darwin-arm64' ? 2 : 1);
  }
  const plan = releasePlan({ root, scope, version });
  assert.equal(plan.length, 7);
  assert.equal(plan.at(-1).name, `${scope}/magicvault`);
  assert(plan.slice(0, 6).every(p => p.kind === 'native'));
  assert.throws(() => releasePlan({ root, scope: '@magicvault-local', version }));
  assert.throws(() => releasePlan({ root, scope, version: '999.0.0' }));
  const original = fs.readFileSync(plan[0].file);
  fs.appendFileSync(plan[0].file, 'changed bytes');
  assert.throws(() => releasePlan({ root, scope, version }), /changed/);
  fs.writeFileSync(plan[0].file, original);
  fs.renameSync(path.join(root, 'MagicVault-alpha-win32-arm64'), path.join(root, 'missing-platform'));
  assert.throws(() => releasePlan({ root, scope, version }));
});

const records = [...platforms.map(p => `${scope}/magicvault-${p}`), `${scope}/magicvault`]
  .map((name, i) => ({ name, version: '0.9.0', file: `/synthetic/${i}.tgz`, integrity: `sha512-synthetic-${i}` }));
function missing() { const e = new Error('synthetic'); e.stdout = JSON.stringify({ error: { code: 'E404' } }); throw e; }
test('registry preflight completes before publication; native packages precede launcher and use alpha', () => {
  const calls = [];
  publishFixture(records, args => { calls.push(args); if (args[0] === 'view') missing(); return ''; });
  assert(calls.slice(0, 7).every(a => a[0] === 'view'));
  assert.deepEqual(calls.slice(7).map(a => a[1]), records.map(p => p.file));
  assert(calls.slice(7).every(a => a.includes('--provenance') && a.includes('--ignore-scripts') && a[a.indexOf('--tag') + 1] === 'alpha'));
});
test('conflicting existing bytes or registry errors abort before any publication', () => {
  for (const fail of ['conflict', 'network']) {
    const calls = [];
    assert.throws(() => publishFixture(records, args => {
      calls.push(args);
      if (args[1].startsWith(records.at(-1).name + '@')) {
        if (fail === 'network') throw new Error('connection refused');
        return JSON.stringify('sha512-other');
      }
      missing();
    }));
    assert(calls.every(a => a[0] === 'view'));
  }
});
test('partial publication resumes only identical already-tagged artifacts; failed publish is never retried', () => {
  const calls = [];
  publishFixture(records, args => {
    calls.push(args);
    if (args[0] === 'view') {
      if (args[1] === `${records[0].name}@0.9.0`) return JSON.stringify(records[0].integrity);
      if (args[1] === records[0].name) return JSON.stringify('0.9.0');
      missing();
    }
    return '';
  });
  assert.deepEqual(calls.filter(a => a[0] === 'publish').map(a => a[1]), records.slice(1).map(r => r.file));
  const wrongTag = [];
  assert.throws(() => publishFixture(records, args => {
    wrongTag.push(args);
    return JSON.stringify(args[2] === 'dist.integrity' ? records[0].integrity : '0.8.0');
  }), /different alpha tag/);
  assert(wrongTag.every(a => a[0] === 'view'));
  let published = 0;
  assert.throws(() => publishFixture(records, args => {
    if (args[0] === 'view') missing();
    if (++published === 2) throw new Error('lost reply');
    return '';
  }));
  assert.equal(published, 2);
});
test('release is manual, artifact-only by default, with credentials confined to publication', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/npm-alpha.yml'), 'utf8');
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /default: false/);
  assert.doesNotMatch(workflow, /^  (?:push|pull_request|release|schedule):/m);
  const beforePublish = workflow.split('\n  publish:\n')[0];
  assert.doesNotMatch(beforePublish, /secrets\.|NODE_AUTH_TOKEN/);
  assert.match(workflow, /if: inputs\.publish && github\.ref == 'refs\/heads\/main'/);
  assert.match(workflow, /NODE_AUTH_TOKEN: \$\{\{ secrets\.NPM_TOKEN \}\}/);
  assert.match(workflow, /needs: \[build, verify\]/);
  for (const platform of platforms) assert(beforePublish.includes(`platform: ${platform},`));
});
