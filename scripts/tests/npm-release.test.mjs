import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { assemble, binaries, platforms, standaloneVersion } from '../package-npm.mjs';
import { packRelease, releasePlan, publishPlan, releaseContext } from '../release-npm.mjs';

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
    assert(readme.includes(`npm install ${scope}/magicvault\n`));
    assert(readme.includes(`from '${scope}/magicvault'`));
    assert.doesNotMatch(readme, /@magicvault-local|not a published npm|\.tgz/);
    for (const osName of ['macOS', 'Linux', 'Windows']) assert(readme.includes(`${osName}-alpha-orange`));
    const entries = packRelease({ packages: output, output: path.join(root, `MagicVault-npm-${platform}`), scope, platform });
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
  fs.renameSync(path.join(root, 'MagicVault-npm-win32-arm64'), path.join(root, 'missing-platform'));
  assert.throws(() => releasePlan({ root, scope, version }));
});

const records = [...platforms.map(p => `${scope}/magicvault-${p}`), `${scope}/magicvault`]
  .map((name, i) => ({ name, version: '0.9.0', file: `/synthetic/${i}.tgz`, integrity: `sha512-synthetic-${i}` }));
function missing() { const e = new Error('synthetic'); e.stdout = JSON.stringify({ error: { code: 'E404' } }); throw e; }
function registry() {
  const calls = [], versions = new Map(), tags = new Map();
  const invoke = args => {
    calls.push(args);
    const [command, name, field] = args;
    if (command === 'view') {
      if (field === 'dist.integrity') {
        if (!versions.has(name)) missing();
        return JSON.stringify(versions.get(name));
      }
      assert.equal(field, 'dist-tags.latest');
      if (![...versions.keys()].some(key => key.startsWith(name + '@'))) missing();
      return tags.has(name) ? JSON.stringify(tags.get(name)) : '';
    }
    if (command === 'publish') {
      const record = records.find(r => r.file === name);
      versions.set(`${record.name}@${record.version}`, record.integrity);
      tags.set(record.name, record.version);
    } else {
      assert.equal(command, 'dist-tag');
      assert.equal(name, 'add');
      const at = field.lastIndexOf('@');
      tags.set(field.slice(0, at), field.slice(at + 1));
    }
    return '';
  };
  return { calls, versions, tags, invoke };
}

test('all four standalone component versions must agree before packaging', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mv-release-versions-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const names = ['magicvault', 'magicvault-mcp', 'magicvault-service', 'magicvault-prompt'];
  const manifest = name => path.join(root, name, 'Cargo.toml');
  for (const name of names) {
    fs.mkdirSync(path.join(root, name));
    fs.writeFileSync(manifest(name), `[package]\nname = "${name}"\nversion = "0.9.0"\n`);
  }
  assert.equal(standaloneVersion(root), '0.9.0');
  for (const name of names) {
    const original = fs.readFileSync(manifest(name));
    fs.writeFileSync(manifest(name), original.toString().replace('0.9.0', '0.9.1'));
    assert.throws(() => standaloneVersion(root), /versions must match/);
    fs.writeFileSync(manifest(name), original);
  }
});

test('only matching tag pushes publish; main pushes and manual preparation cannot publish', () => {
  const base = { scope, version: '0.9.0', event: 'push', ref: 'refs/tags/v0.9.0' };
  assert.deepEqual(releaseContext(base), { scope, version: '0.9.0', tag: 'latest', publish: true });
  assert.equal(releaseContext({ ...base, ref: 'refs/heads/main' }).publish, false);
  for (const ref of ['refs/heads/unreviewed', 'refs/tags/v0.8.3', 'refs/tags/v0.9.0-alpha.1', 'refs/tags/v00.9.0', 'refs/tags/v0.9.0\n']) {
    assert.throws(() => releaseContext({ ...base, ref }));
  }
  for (const ref of ['refs/heads/main', base.ref]) {
    assert.equal(releaseContext({ ...base, event: 'workflow_dispatch', ref }).publish, false);
  }
  for (const patch of [{ scope: '' }, { scope: '@magicvault-local' }, { scope: '@scope\npublish=true' },
    { version: '0.9.0-alpha.1' }, { version: '0.9.0\n', ref: 'refs/tags/v0.9.0\n' },
    { event: 'pull_request' }, { event: 'workflow_dispatch', ref: 'refs/heads/unreviewed' }]) {
    assert.throws(() => releaseContext({ ...base, ...patch }));
  }
});

test('registry preflight completes before publication; native packages precede launcher and verify latest', () => {
  const r = registry();
  publishFixture(records, r.invoke);
  const firstWrite = r.calls.findIndex(args => args[0] !== 'view');
  assert.equal(firstWrite, records.length * 2);
  const uploads = r.calls.filter(args => args[0] === 'publish');
  assert.deepEqual(uploads.map(a => a[1]), records.map(p => p.file));
  assert(uploads.every(a => a.includes('--provenance') && a.includes('--ignore-scripts') && a[a.indexOf('--tag') + 1] === 'latest'));
  assert.equal(r.calls.slice(firstWrite + records.length).length, records.length * 2);
  assert([...r.tags.values()].every(v => v === '0.9.0'));
});

test('conflicting existing bytes, newer latest, malformed and unavailable registry replies abort before publication', () => {
  for (const fail of ['conflict', 'newer', 'network', 'malformed']) {
    const r = registry();
    const last = records.at(-1);
    if (fail === 'conflict') r.versions.set(`${last.name}@0.9.0`, 'sha512-other');
    if (fail === 'newer') { r.versions.set(`${last.name}@0.10.0`, 'newer'); r.tags.set(last.name, '0.10.0'); }
    assert.throws(() => publishFixture(records, args => {
      if (args[0] === 'view' && args[1] === `${last.name}@0.9.0`) {
        if (fail === 'network') throw new Error('connection refused');
        if (fail === 'malformed') return 'not JSON';
      }
      return r.invoke(args);
    }));
    assert(r.calls.every(a => a[0] === 'view'));
  }
});

test('partial publication resumes identical bytes and can repair a missing latest tag', () => {
  for (const latest of ['0.9.0', '0.8.3', undefined]) {
    const r = registry();
    r.versions.set(`${records[0].name}@0.9.0`, records[0].integrity);
    if (latest !== undefined) r.tags.set(records[0].name, latest);
    publishFixture(records, r.invoke);
    assert.deepEqual(r.calls.filter(a => a[0] === 'publish').map(a => a[1]), records.slice(1).map(p => p.file));
    assert.equal(r.calls.filter(a => a[0] === 'dist-tag').length, latest === '0.9.0' ? 0 : 1);
  }
});

test('uncertain upload stops immediately; completed runs are idempotent', () => {
  const r = registry(); let uploads = 0;
  assert.throws(() => publishFixture(records, args => {
    const result = r.invoke(args);
    if (args[0] === 'publish' && ++uploads === 2) throw new Error('lost reply after upload');
    return result;
  }));
  assert.equal(uploads, 2);
  publishFixture(records, r.invoke);
  const writes = r.calls.filter(a => a[0] !== 'view').length;
  publishFixture(records, r.invoke);
  assert.equal(r.calls.filter(a => a[0] !== 'view').length, writes);
});

test('post-publication readback detects a registry tag that did not take effect', () => {
  const r = registry();
  assert.throws(() => publishFixture(records, args => {
    const result = r.invoke(args);
    if (args[0] === 'publish') r.tags.delete(records.find(p => p.file === args[1]).name);
    return result;
  }), /post-publication/);
});

test('tag publication is gated by preparation, source checks, all builds and artifact verification', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/npm-release.yml'), 'utf8');
  assert.match(workflow, /tags: \['v\[0-9\]\*'\]/);
  assert.match(workflow, /branches: \[main\]/);
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /inputs\.npm_scope \|\| '@magicbeansai'/);
  assert.match(workflow, /git merge-base --is-ancestor/);
  assert.match(workflow, /uses: \.\/\.github\/workflows\/ci\.yml/);
  assert.match(workflow, /build:\n    needs: \[prepare, checks\]/);
  const beforePublish = workflow.split('\n  publish:\n')[0];
  assert.doesNotMatch(beforePublish, /secrets\.|NODE_AUTH_TOKEN/);
  assert.match(workflow, /if: needs\.prepare\.outputs\.publish == 'true'/);
  assert.match(workflow, /NODE_AUTH_TOKEN: \$\{\{ secrets\.NPM_TOKEN \}\}/);
  assert.match(workflow, /needs: \[prepare, checks, build, verify\]/);
  for (const platform of platforms) assert(beforePublish.includes(`platform: ${platform},`));
});
