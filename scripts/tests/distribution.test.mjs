import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { createHash, createPublicKey } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { assemble, binaries } from '../package-npm.mjs';

const require = createRequire(import.meta.url);
const { resolveBinary } = require('../../npm/launcher.cjs');
const { fixture: extensionFixture, options: extensionOptions, tick } = require('../../extension/tests/harness.cjs');
const repo = path.resolve(import.meta.dirname, '../..');
test('browser resource observation bounds are checked before creating artifacts', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-resource-optin-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const work = path.join(root, 'must-not-exist');
  for (const args of [
    ['--browser-idle-secs', '120'],
    ...['0', '29', '301', '1e2', '-1', '30.0', ' 30', ''].map(n => ['--with-browser-tests', `--browser-idle-secs=${n}`]),
    ['--with-browser-tests', '--browser-idle-secs', '120', '--reliability-rounds', '2'],
  ]) {
    assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
      '--packages', root, '--work', work, ...args], {stdio: 'pipe', timeout: 5000}),
    error => error.status !== 0 && String(error.stderr).includes('browser resource observations require'));
    assert.equal(fs.existsSync(work), false);
  }
  for (const n of ['30', '300']) {
    const env = {...process.env, MAGICVAULT_CHROME: ''};
    delete env.MAGICVAULT_TEST_STARTUP_DIAGNOSTICS;
    assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
      '--packages', root, '--work', work, '--with-rust-tests', '--with-browser-tests', `--browser-idle-secs=${n}`],
    {env, stdio: 'pipe', timeout: 5000}), error => error.status !== 0 && String(error.stderr).includes('browser qualification requires'));
    assert.equal(fs.existsSync(work), false);
  }
  assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
    '--packages', root, '--work', work, '--with-rust-tests', '--with-browser-tests', '--browser-idle-secs=120'],
  {env: {...process.env, MAGICVAULT_TEST_STARTUP_DIAGNOSTICS: ''}, stdio: 'pipe', timeout: 5000}),
  error => error.status !== 0 && String(error.stderr).includes('browser resource observations require'));
  assert.equal(fs.existsSync(work), false);
});
test('process diagnostic driver refuses implicit instrumentation and ambient compiler flags', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-diagnostic-optin-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const work = path.join(root, 'must-not-exist');
  for (const [args, extra] of [[[], {}], [['--with-rust-tests'], {RUSTFLAGS: '--cfg unrelated'}], [['--with-rust-tests'], {CARGO_ENCODED_RUSTFLAGS: '--cfg\x1funrelated'}]]) {
    assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
      '--packages', root, '--work', work, '--with-process-diagnostics', ...args], {
      env: {...process.env, ...extra}, stdio: 'pipe', timeout: 5000,
    }), error => error.status !== 0 && String(error.stderr).includes('process diagnostics require'));
    assert.equal(fs.existsSync(work), false);
  }
});
test('real package browser qualification requires explicit prerequisites before creating artifacts', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-browser-optin-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const work = path.join(root, 'must-not-exist');
  for (const [extra, browser] of [[[], process.execPath], [['--with-rust-tests'], 'relative-chrome']]) {
    assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
      '--packages', root, '--work', work, '--app-parent', root, '--with-browser-tests', ...extra], {
      env: {...process.env, MAGICVAULT_CHROME: browser}, stdio: 'pipe', timeout: 5000,
    }), error => error.status !== 0 && String(error.stderr).includes('browser qualification requires'));
    assert.equal(fs.existsSync(work), false);
  }
});
test('browser qualification never implicitly installs its native host on the artifact volume', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-browser-app-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const work = path.join(root, 'must-not-exist');
  assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
    '--packages', root, '--work', work, '--with-rust-tests', '--with-browser-tests'], {
    env: {...process.env, MAGICVAULT_CHROME: process.execPath}, stdio: 'pipe', timeout: 5000,
  }), error => error.status !== 0 && String(error.stderr).includes('requires explicit --app-parent'));
  assert.equal(fs.existsSync(work), false);
});
test('reliability qualification rejects unbounded or implicit workloads before creating artifacts', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-reliability-options-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const work = path.join(root, 'must-not-exist');
  for (const args of [
    ['--reliability-rounds', '0', '--with-rust-tests'],
    ['--reliability-rounds', '21', '--with-rust-tests'],
    ['--reliability-rounds', '2.5', '--with-rust-tests'],
    ['--reliability-rounds', '02', '--with-rust-tests'],
    ['--reliability-rounds', '2'], ['--with-performance-tests'],
    ['--app-parent', 'relative'],
  ]) {
    assert.throws(() => execFileSync(process.execPath, [path.join(repo, 'scripts/qualify-package.mjs'),
      '--packages', root, '--work', work, ...args], { stdio: 'pipe', timeout: 5000 }),
    error => error.status !== 0 && /reliability qualification requires|app-parent must/.test(String(error.stderr)));
    assert.equal(fs.existsSync(work), false);
  }
});
test('bundled public identity matches the exact native-host default and is a valid public key', () => {
  const manifest = JSON.parse(fs.readFileSync(path.join(repo, 'extension/manifest.json')));
  const der = Buffer.from(manifest.key, 'base64');
  assert.equal(createPublicKey({key: der, format: 'der', type: 'spki'}).type, 'public');
  const id = [...createHash('sha256').update(der).digest().subarray(0, 16).toString('hex')]
    .map(x => String.fromCharCode(97 + parseInt(x, 16))).join('');
  const native = fs.readFileSync(path.join(repo, 'magicvault-service/src/native.rs'), 'utf8');
  assert(native.includes(`pub const EXTENSION_ID: &str = "${id}";`));
  assert(manifest.permissions.includes('alarms'));
});
function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-package-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const bin = path.join(root, 'bin');
  fs.mkdirSync(bin);
  for (const name of binaries) {
    const bytes = Buffer.alloc(64);
    bytes.writeUInt32LE(0xfeedfacf); bytes.writeUInt32LE(0x0100000c, 4);
    fs.writeFileSync(path.join(bin, name), bytes);
  }
  const output = path.join(root, 'packages');
  const build = () => assemble({ repo, binaryDir: bin, output, scope: '@magicvault-local' });
  return { root, bin, output, build };
}
function install(f) {
  f.build();
  const modules = path.join(f.output, 'launcher/node_modules/@magicvault-local');
  fs.mkdirSync(modules, { recursive: true });
  fs.renameSync(path.join(f.output, 'native'), path.join(modules, 'magicvault-darwin-arm64'));
  return { main: path.join(f.output, 'launcher/package.json'), native: path.join(modules, 'magicvault-darwin-arm64') };
}
test('assembly includes only explicit assets and exact platform dependency, with no install hooks', t => {
  const f = fixture(t); f.build();
  const main = JSON.parse(fs.readFileSync(path.join(f.output, 'launcher/package.json')));
  const native = JSON.parse(fs.readFileSync(path.join(f.output, 'native/package.json')));
  assert.equal(main.description, 'Let agents use credentials without seeing them — reference-only credential delivery');
  assert.match(fs.readFileSync(path.join(f.output, 'launcher/README.md'), 'utf8'), /\*\*Let agents use credentials without seeing them\*\*/);
  assert.equal(main.optionalDependencies[native.name], native.version);
  assert.equal(main.scripts, undefined); assert.equal(native.scripts, undefined);
  assert.deepEqual(native.os, ['darwin']); assert.deepEqual(native.cpu, ['arm64']);
  const manifest = JSON.parse(fs.readFileSync(path.join(f.output, 'native/bundle.json')));
  assert.equal(Object.keys(manifest.files).length, 14);
  assert.equal(fs.existsSync(path.join(f.output, 'native/Cargo.lock')), false);
  assert.throws(f.build); // Cannot overwrite an output directory.
});
test('assembly refuses binary symlinks and wrong architecture before producing output', t => {
  const f = fixture(t);
  fs.unlinkSync(path.join(f.bin, 'magicvault'));
  fs.symlinkSync(path.join(f.bin, 'magicvault-mcp'), path.join(f.bin, 'magicvault'));
  assert.throws(f.build); assert.equal(fs.existsSync(f.output), false);
  fs.unlinkSync(path.join(f.bin, 'magicvault'));
  fs.writeFileSync(path.join(f.bin, 'magicvault'), 'not a Mach-O');
  assert.throws(f.build);
});

test('assembled extension boots actual worker imports and wires site-access setup', async t => {
  const f = fixture(t); f.build();
  const directory = path.join(f.output, 'native/extension');
  const extension = extensionFixture({directory});
  const ui = extensionOptions(extension); await tick();
  await ui.click('allow-all'); assert(extension.grants.has('https://*/*'));
  ui.element('origin').value = 'https://example.com'; await ui.click('block');
  assert.deepEqual(extension.stored.sitePolicy.blockedSites, ['https://example.com']);
  fs.unlinkSync(path.join(directory, 'fill.js'));
  assert.throws(() => extensionFixture({directory}), /ENOENT/, 'a missing imported asset must fail startup');
});
test('launcher verifies exact version and bytes; unsupported platforms fail closed', t => {
  const f = fixture(t); const p = install(f);
  assert.equal(resolveBinary('magicvault', p.main, 'darwin', 'arm64'), fs.realpathSync(path.join(p.native, 'bin/magicvault')));
  assert.throws(() => resolveBinary('magicvault', p.main, 'linux', 'arm64'));
  assert.throws(() => resolveBinary('magicvault-native-host', p.main, 'darwin', 'arm64'));
  fs.appendFileSync(path.join(p.native, 'bin/magicvault'), 'tamper');
  assert.throws(() => resolveBinary('magicvault', p.main, 'darwin', 'arm64'));
  const file = path.join(p.native, 'package.json');
  const metadata = JSON.parse(fs.readFileSync(file)); metadata.version = '99.0.0';
  fs.writeFileSync(file, JSON.stringify(metadata));
  assert.throws(() => resolveBinary('magicvault-mcp', p.main, 'darwin', 'arm64'));
});
test('launcher refuses a native executable symlink', t => {
  const f = fixture(t); const p = install(f);
  fs.unlinkSync(path.join(p.native, 'bin/magicvault'));
  fs.symlinkSync(path.join(p.native, 'bin/magicvault-mcp'), path.join(p.native, 'bin/magicvault'));
  assert.throws(() => resolveBinary('magicvault', p.main, 'darwin', 'arm64'));
});

test('FIFO and oversized metadata are refused without unbounded reads', t => {
  const f = fixture(t); const p = install(f);
  const file = path.join(p.native, 'bundle.json');
  fs.writeFileSync(file, ' '.repeat(64 * 1024 + 1));
  assert.throws(() => resolveBinary('magicvault', p.main, 'darwin', 'arm64'));
  fs.unlinkSync(file);
  execFileSync('/usr/bin/mkfifo', [file]);
  assert.throws(() => resolveBinary('magicvault', p.main, 'darwin', 'arm64'));
});

test('signing helper parses and rejects relative paths before any signing action', () => {
  const script = path.join(repo, 'scripts/sign-release.sh');
  execFileSync('/bin/sh', ['-n', script]);
  assert.throws(() => execFileSync('/bin/sh', [script, 'relative'], { stdio: 'pipe' }));
});

test('workflow initializes runner paths at step scope and exports them to later steps', t => {
  const workflow = fs.readFileSync(path.join(repo, '.github/workflows/distribution.yml'), 'utf8');
  const jobEnvironment = workflow.match(/^    env:\n([\s\S]*?)^    steps:/m)?.[1];
  assert.ok(jobEnvironment, 'expected job environment');
  assert.doesNotMatch(jobEnvironment, /\$\{\{\s*(?:runner|env)\b/);
  const initializer = workflow.match(/^      - name: Initialize artifact locations\n        shell: bash\n        run: \|\n((?:          [^\n]*\n)+)/m);
  assert.ok(initializer, 'expected runner-time path initialization');
  assert(initializer.index < workflow.indexOf('- uses:'), 'initialize before checkout/build/package steps');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-workflow-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const runnerTemp = path.join(root, 'runner temp with spaces');
  const environmentFile = path.join(root, 'github env');
  const script = initializer[1].replace(/^          /gm, '');
  execFileSync('/bin/bash', ['-e', '-u', '-o', 'pipefail', '-c', script], {
    env: { RUNNER_TEMP: runnerTemp, GITHUB_ENV: environmentFile }, stdio: 'pipe',
  });
  assert.deepEqual(fs.readFileSync(environmentFile, 'utf8').trimEnd().split('\n'), [
    `CARGO_TARGET_DIR=${runnerTemp}/magicvault-builds`,
    `PACKAGE_PARENT=${runnerTemp}/magicvault-distribution`,
  ]);
  assert(workflow.includes('path: ${{ env.PACKAGE_PARENT }}/qualification/*.tgz'));
  assert.equal(fs.existsSync(runnerTemp), false, 'initialization exports paths without creating artifacts');
});
