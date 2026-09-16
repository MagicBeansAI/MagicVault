import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createRequire } from 'node:module';
import { assemble, binaries, platforms, standaloneVersion } from '../package-npm.mjs';
import { packRelease, assembleRelease, releasePlan, publishPlan, releaseContext, retireLegacy, removeLegacy, npm } from '../release-npm.mjs';

const repo = path.resolve(import.meta.dirname, '../..');
const scope = '@magicvault-release-fixture';
const publishFixture = (plan, invoke) => publishPlan(plan, invoke, () => {}, () => {});
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
    assert.equal(entries.length, 1);
  }
  const output = path.join(root, 'universal');
  assembleRelease({root, output, scope, version});
  const finalRoot = path.join(output, 'tarballs');
  const plan = releasePlan({ root: finalRoot, scope, version });
  assert.equal(plan.length, 1);
  const metadata = JSON.parse(fs.readFileSync(path.join(output, 'package/package.json')));
  assert.equal(metadata.optionalDependencies, undefined);
  assert.equal(metadata.private, undefined);
  assert.equal(metadata.scripts, undefined);
  assert.deepEqual(Object.keys(metadata.magicvault.platforms), platforms);
  for (const platform of platforms) assert(fs.existsSync(path.join(output, 'package/native', platform, 'bundle.json')));
  assert.equal(plan.at(-1).name, `${scope}/magicvault`);
  assert.equal(plan[0].kind, 'universal');
  const install = path.join(root, 'installed');
  npm(['install', '--prefix', install, '--offline', '--ignore-scripts', '--omit=optional', '--no-audit', '--no-fund', plan[0].file]);
  assert.deepEqual(fs.readdirSync(path.join(install, 'node_modules', scope)), ['magicvault']);
  const require = createRequire(import.meta.url);
  const {resolveBinary} = require('../../npm/launcher.cjs');
  const packageFile = path.join(install, 'node_modules', scope, 'magicvault/package.json');
  for (const platform of platforms) {
    const [system, arch] = platform.split('-');
    assert.match(resolveBinary('magicvault', packageFile, system, arch), /native/);
  }
  assert.throws(() => resolveBinary('magicvault', packageFile, 'freebsd', 'x64'));
  assert.throws(() => resolveBinary('magicvault', packageFile, 'linux', 'ia32'));

  assert.throws(() => releasePlan({ root, scope: '@magicvault-local', version }));
  assert.throws(() => releasePlan({ root, scope, version: '999.0.0' }));
  const original = fs.readFileSync(plan[0].file);
  fs.appendFileSync(plan[0].file, 'changed bytes');
  assert.throws(() => releasePlan({ root: finalRoot, scope, version }), /changed/);
  fs.writeFileSync(plan[0].file, original);
  fs.renameSync(path.join(root, 'MagicVault-npm-win32-arm64'), path.join(root, 'missing-platform'));
  assert.throws(() => assembleRelease({root, output: path.join(root, 'incomplete'), scope, version}));
});

const records = [`${scope}/magicvault`]
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

test('registry preflight completes before publishing the single package and verifying latest', () => {
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
    if (args[0] === 'publish' && ++uploads === 1) throw new Error('lost reply after upload');
    return result;
  }));
  assert.equal(uploads, 1);
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
  assert.match(workflow, /needs: \[prepare, checks, build, verify, smoke\]/);
  for (const platform of platforms) assert(beforePublish.includes(`platform: ${platform},`));
});

test('registry visibility delays retry reads without repeating an upload', () => {
  const r = registry(); let afterUpload = false, staleReads = 0;
  publishFixture(records, args => {
    if (args[0] === 'publish') afterUpload = true;
    if (afterUpload && args[0] === 'view' && staleReads++ < 2) missing();
    return r.invoke(args);
  });
  assert.equal(r.calls.filter(a => a[0] === 'publish').length, 1);
});

function migrationRegistry() {
  const scope = '@magicbeansai', version = '0.9.1';
  const calls = [], deprecated = new Map();
  const metadata = { name: `${scope}/magicvault`, version,
    publishConfig: {access: 'public', tag: 'latest', registry: 'https://registry.npmjs.org/'},
    repository: {url: 'git+https://github.com/MagicBeansAI/MagicVault.git'},
    magicvault: {layout: 'bundled-v1', platforms: Object.fromEntries([...platforms].reverse().map(p => [p, `native/${p}`]))},
    os: [...new Set(platforms.map(p => p.split('-')[0]))], cpu: [...new Set(platforms.map(p => p.split('-')[1]))],
    bin: {magicvault: 'cli.cjs', 'magicvault-mcp': 'mcp.cjs'}, main: './sdk.cjs', types: './sdk.d.cts' };
  const invoke = args => {
    calls.push(args);
    const [command, name, field] = args;
    if (command === 'view') {
      if (field === '--json') return JSON.stringify(metadata);
      if (field === 'dist-tags.latest') return JSON.stringify(version);
      if (field === 'versions') return JSON.stringify(['0.9.0']);
      if (field === 'version') return JSON.stringify('0.9.0');
      assert.equal(field, 'deprecated');
      return deprecated.has(name) ? JSON.stringify(deprecated.get(name)) : '';
    }
    assert.equal(command, 'deprecate');
    deprecated.set(name.endsWith('@0.9.0') ? name : `${name}@0.9.0`, field);
    return '';
  };
  return {scope, version, calls, metadata, deprecated, invoke};
}

test('legacy migration requires the bundled replacement and preserves all old downloads', () => {
  const r = migrationRegistry();
  retireLegacy(r, r.invoke, () => {}, () => {});
  assert.equal(r.deprecated.size, 7);
  assert(r.calls.every(a => ['view', 'deprecate'].includes(a[0])));
  assert(r.calls.findIndex(a => a[0] === 'deprecate') >= 9);
  const writes = r.calls.filter(a => a[0] === 'deprecate').length;
  retireLegacy(r, r.invoke, () => {}, () => {});
  assert.equal(r.calls.filter(a => a[0] === 'deprecate').length, writes);
});

test('migration refuses the wrong scope, a dependent replacement, non-latest release or unexpected legacy versions before writing', () => {
  for (const failure of ['scope', 'version', 'dependencies', 'latest', 'legacy']) {
    const r = migrationRegistry();
    if (failure === 'scope') r.scope = '@other-owner';
    if (failure === 'version') r.version = '0.9.2';
    if (failure === 'dependencies') r.metadata.optionalDependencies = {};
    assert.throws(() => retireLegacy(r, args => {
      if (args[0] === 'view' && args[2] === 'dist-tags.latest' && failure === 'latest') return '"0.9.0"';
      if (args[0] === 'view' && args[2] === 'versions' && failure === 'legacy') return '["0.9.0","0.9.1"]';
      return r.invoke(args);
    }, () => {}, () => {}));
    assert(r.calls.every(a => a[0] === 'view'));
  }
});


test('publication tolerates a full five-minute registry cache lifetime without uploading twice', () => {
  const r = registry(); let published = false, elapsed = 0;
  publishPlan(records, args => {
    if (args[0] === 'publish') published = true;
    if (published && args[0] === 'view' && elapsed < 300_000) missing();
    return r.invoke(args);
  }, () => {}, ms => { assert(ms <= 60_000); elapsed += ms; });
  assert(elapsed >= 300_000 && elapsed <= 310_000);
  assert.equal(r.calls.filter(a => a[0] === 'publish').length, 1);
});

function removalRegistry() {
  const old = migrationRegistry(), version = '0.9.2', calls = [];
  const metadata = {...old.metadata, version, dist: {integrity: 'sha512-replacement'}};
  const remaining = new Set(platforms.map(p => `@magicbeansai/magicvault-${p}`));
  const invoke = args => {
    calls.push(args);
    const [command, spec, field] = args;
    if (command === 'unpublish') {
      assert(spec.endsWith('@0.9.0'));
      assert(remaining.delete(spec.slice(0, -6)), 'only known native packages can be removed');
      assert(args.includes('--force') && args.includes('--ignore-scripts'));
      return '';
    }
    assert.equal(command, 'view');
    if (spec === '@magicbeansai/magicvault') {
      assert.equal(field, 'dist-tags.latest'); return JSON.stringify(version);
    }
    if (spec === `@magicbeansai/magicvault@${version}`) {
      return JSON.stringify(field === '--json' ? metadata : metadata.dist.integrity);
    }
    const name = spec.endsWith('@0.9.0') ? spec.slice(0, -6) : spec;
    if (!remaining.has(name)) missing();
    if (field === 'versions') return '["0.9.0"]';
    if (field === 'deprecated') return '"Retired compatibility package"';
    assert.equal(field, 'version'); return '"0.9.0"';
  };
  return {scope: old.scope, version, metadata, remaining, calls, invoke};
}

test('removal deletes exactly six native versions, preserves main and skips completed removals', () => {
  const r = removalRegistry();
  removeLegacy(r, r.invoke, () => {}, () => {});
  const writes = r.calls.filter(a => a[0] === 'unpublish');
  assert.deepEqual(writes.map(a => a[1]), platforms.map(p => `@magicbeansai/magicvault-${p}@0.9.0`));
  assert.equal(r.remaining.size, 0);
  assert(r.calls.findIndex(a => a[0] === 'unpublish') >= 14);
  removeLegacy(r, r.invoke, () => {}, () => {});
  assert.equal(r.calls.filter(a => a[0] === 'unpublish').length, 6);
});

test('removal preflights all targets and refuses unsafe replacement or unexpected registry state before deleting', () => {
  for (const failure of ['scope', 'version', 'dependencies', 'integrity', 'latest', 'legacy', 'deprecated', 'network']) {
    const r = removalRegistry();
    if (failure === 'scope') r.scope = '@other-owner';
    if (failure === 'version') r.version = '0.9.3';
    if (failure === 'dependencies') r.metadata.optionalDependencies = {};
    if (failure === 'integrity') delete r.metadata.dist;
    assert.throws(() => removeLegacy(r, args => {
      if (args[0] === 'view') {
        if (failure === 'latest' && args[2] === 'dist-tags.latest') return '"0.9.1"';
        if (args[1].includes('win32-arm64')) {
          if (failure === 'legacy' && args[2] === 'versions') return '["0.9.0","0.9.1"]';
          if (failure === 'deprecated' && args[2] === 'deprecated') return '';
          if (failure === 'network') throw new Error('connection refused');
        }
      }
      return r.invoke(args);
    }, () => {}, () => {}), failure);
    assert(r.calls.every(a => a[0] === 'view'), failure);
  }
});

test('uncertain removal stops without retry and a later rerun resumes remaining packages', () => {
  const r = removalRegistry(); let writes = 0;
  assert.throws(() => removeLegacy(r, args => {
    const result = r.invoke(args);
    if (args[0] === 'unpublish' && ++writes === 1) throw new Error('lost reply');
    return result;
  }, () => {}, () => {}), /removal failed/);
  assert.equal(writes, 1);
  assert.equal(r.remaining.size, 5);
  removeLegacy(r, r.invoke, () => {}, () => {});
  assert.equal(r.calls.filter(a => a[0] === 'unpublish').length, 6);
});

test('npm policy refusal is reported without deleting more packages or echoing diagnostics', () => {
  const r = removalRegistry(), reports = []; let writes = 0;
  assert.throws(() => removeLegacy(r, args => {
    if (args[0] === 'unpublish') {
      writes++;
      const e = new Error('sensitive diagnostic');
      e.stdout = JSON.stringify({error: {code: 'E400', summary: 'dependent packages', detail: 'private data'}});
      throw e;
    }
    return r.invoke(args);
  }, line => reports.push(line), () => {}));
  assert.equal(writes, 1);
  assert.equal(r.remaining.size, 6);
  assert.match(reports.join('\n'), /E400 \(npm reports dependent packages\)/);
  assert.doesNotMatch(reports.join('\n'), /private data|sensitive diagnostic/);
});

test('unpublish cleanup can run only after successful tagged 0.9.2 publication', () => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/npm-release.yml'), 'utf8');
  const cleanup = workflow.split('\n  remove-legacy:\n')[1];
  assert.match(cleanup, /needs: \[prepare, publish\]/);
  assert.match(cleanup, /if: needs\.prepare\.outputs\.publish == 'true' && needs\.prepare\.outputs\.version == '0.9.2' && needs\.prepare\.outputs\.scope == '@magicbeansai'/);
  assert.match(cleanup, /--mode remove-legacy --scope @magicbeansai --version 0.9.2/);
});
