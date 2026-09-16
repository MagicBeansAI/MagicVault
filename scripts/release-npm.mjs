#!/usr/bin/env node
// Local packing/verification by default. Only --mode publish contacts npm.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { platforms, assets, binaries } from './package-npm.mjs';

const registry = 'https://registry.npmjs.org/';
const anchor = 'darwin-arm64'; // Exactly one runner produces the common launcher.
const launcherFiles = ['package.json', 'README.md', 'cli.cjs', 'mcp.cjs', 'launcher.cjs', 'sdk.cjs', 'sdk.d.cts', 'LICENSE-MIT', 'LICENSE-APACHE'];
const digest = bytes => `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`;
function ensure(ok, message) { if (!ok) throw new Error(message); }
function identity(scope, version) {
  ensure(typeof scope === 'string' && scope.trim() === scope && /^@[a-z0-9][a-z0-9-]{0,63}$/.test(scope) && scope !== '@magicvault-local', 'select an owned public npm scope');
  ensure(typeof version === 'string' && version.trim() === version && /^\d+\.\d+\.\d+$/.test(version), 'invalid release version');
}
function read(file, max = 65536) {
  const stat = fs.lstatSync(file);
  ensure(stat.isFile() && stat.size <= max, 'invalid release artifact');
  return fs.readFileSync(file);
}
const json = file => JSON.parse(read(file));
function npmCli() {
  const candidates = [process.env.npm_execpath,
    path.join(path.dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'),
    path.resolve(path.dirname(process.execPath), '../lib/node_modules/npm/bin/npm-cli.js')];
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    const file = path.join(dir, process.platform === 'win32' ? 'npm.cmd' : 'npm');
    if (fs.existsSync(file)) candidates.push(process.platform === 'win32'
      ? path.join(dir, 'node_modules/npm/bin/npm-cli.js') : fs.realpathSync(file));
  }
  const found = candidates.find(file => file && path.isAbsolute(file) && path.basename(file) === 'npm-cli.js' && fs.existsSync(file));
  ensure(found, 'npm CLI must be installed beside Node or on PATH');
  return found;
}
function npm(args, options = {}) {
  return execFileSync(process.execPath, [npmCli(), ...args], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000, maxBuffer: 1024 * 1024, ...options,
  });
}
function checkMetadata(meta, scope, version, platform) {
  ensure(meta.name === `${scope}/magicvault${platform ? `-${platform}` : ''}` && meta.version === version, 'package identity mismatch');
  ensure(!Object.hasOwn(meta, 'scripts') && meta.publishConfig?.access === 'public'
    && meta.publishConfig?.tag === 'alpha' && meta.publishConfig?.registry === registry, 'unsafe publication metadata');
  ensure(Object.keys(meta.publishConfig).sort().join(',') === 'access,registry,tag'
    && !['dependencies', 'devDependencies', 'peerDependencies', 'bundledDependencies', 'bundleDependencies'].some(k => Object.hasOwn(meta, k)), 'unexpected package dependencies or publication overrides');
  ensure(meta.repository?.url === 'git+https://github.com/MagicBeansAI/MagicVault.git', 'wrong source repository');
  if (platform) {
    const [os, cpu] = platform.split('-');
    ensure(JSON.stringify(meta.os) === JSON.stringify([os]) && JSON.stringify(meta.cpu) === JSON.stringify([cpu]), 'native platform mismatch');
    ensure(!Object.hasOwn(meta, 'optionalDependencies'), 'native package must be self-contained');
  } else {
    const expected = Object.fromEntries(platforms.map(p => [`${scope}/magicvault-${p}`, version]));
    ensure(JSON.stringify(meta.optionalDependencies) === JSON.stringify(expected), 'incomplete native dependency set');
    ensure(platforms.every(p => meta.magicvault?.platforms?.[p] === `${scope}/magicvault-${p}`), 'wrong launcher platform map');
    ensure(meta.bin?.magicvault === 'cli.cjs' && meta.bin?.['magicvault-mcp'] === 'mcp.cjs'
      && Object.keys(meta.bin).length === 2 && meta.main === './sdk.cjs' && meta.types === './sdk.d.cts', 'wrong public entry points');
  }
}

export function packRelease({ packages, output, scope, platform }) {
  ensure(platforms.includes(platform), 'unsupported release platform');
  const meta = json(path.join(packages, 'native/package.json'));
  identity(scope, meta.version);
  checkMetadata(meta, scope, meta.version, platform);
  checkMetadata(json(path.join(packages, 'launcher/package.json')), scope, meta.version);
  fs.mkdirSync(output); // Caller supplies a fresh output; no overwrite or reuse.
  const entries = [];
  for (const kind of platform === anchor ? ['native', 'launcher'] : ['native']) {
    const [packed] = JSON.parse(npm(['pack', path.resolve(packages, kind), '--json', '--ignore-scripts', '--offline', '--pack-destination', path.resolve(output)]));
    const expectedName = `${scope}/magicvault${kind === 'native' ? `-${platform}` : ''}`;
    ensure(packed.name === expectedName && packed.version === meta.version && path.basename(packed.filename) === packed.filename, 'unexpected packed identity');
    const integrity = digest(read(path.join(output, packed.filename), 512 * 1024 * 1024));
    ensure(integrity === packed.integrity, 'packed integrity mismatch');
    entries.push({ name: packed.name, version: packed.version, filename: packed.filename, integrity, kind });
  }
  fs.writeFileSync(path.join(output, 'release.json'), JSON.stringify({ schema: 1, scope, version: meta.version, platform, entries }, null, 2) + '\n', { flag: 'wx' });
  return entries;
}

function archiveMetadata(file, platform) {
  // Read only: never extract archive paths into the workspace or run its code.
  const args = { encoding: 'utf8', timeout: 30000, maxBuffer: 65536, stdio: ['ignore', 'pipe', 'pipe'] };
  const listing = execFileSync('tar', ['-tzf', file], args).trim().split('\n');
  const suffix = platform?.startsWith('win32-') ? '.exe' : '';
  const expected = (platform ? ['package.json', 'bundle.json', ...Object.keys(assets), ...binaries.map(b => `bin/${b}${suffix}`)] : launcherFiles).map(p => `package/${p}`).sort();
  ensure(JSON.stringify(listing.sort()) === JSON.stringify(expected), 'unexpected files in release tarball');
  return JSON.parse(execFileSync('tar', ['-xOf', file, 'package/package.json'], args));
}

export function releasePlan({ root, scope, version }) {
  identity(scope, version);
  const records = [];
  for (const platform of platforms) {
    const directory = path.join(root, `MagicVault-alpha-${platform}`);
    const manifest = json(path.join(directory, 'release.json'));
    ensure(manifest.schema === 1 && manifest.scope === scope && manifest.version === version && manifest.platform === platform, 'release set identity mismatch');
    const kinds = platform === anchor ? ['native', 'launcher'] : ['native'];
    ensure(Array.isArray(manifest.entries) && manifest.entries.length === kinds.length, 'incomplete release set');
    for (let i = 0; i < kinds.length; i++) {
      const record = manifest.entries[i];
      const native = kinds[i] === 'native';
      const name = `${scope}/magicvault${native ? `-${platform}` : ''}`;
      const filename = `${name.slice(1).replace('/', '-')}-${version}.tgz`;
      ensure(record.kind === kinds[i] && record.name === name && record.version === version && record.filename === filename, 'invalid release record');
      const file = path.join(directory, filename);
      ensure(digest(read(file, 512 * 1024 * 1024)) === record.integrity, 'release tarball changed');
      checkMetadata(archiveMetadata(file, native ? platform : undefined), scope, version, native ? platform : undefined);
      records.push({ ...record, file });
    }
  }
  return [...records.filter(p => p.kind === 'native'), ...records.filter(p => p.kind === 'launcher')];
}

// All registry preflights finish before the first mutation. An ambiguous publish
// failure stops immediately; a later explicit run can resume identical artifacts.
export function publishPlan(plan, invoke = npm, report = console.log) {
  const states = plan.map(record => {
    let integrity;
    try { integrity = JSON.parse(invoke(['view', `${record.name}@${record.version}`, 'dist.integrity', '--json', '--registry', registry])); }
    catch (error) {
      let code;
      try { code = JSON.parse(String(error.stdout)).error?.code; } catch { /* closed below */ }
      ensure(code === 'E404', 'registry preflight failed; nothing published');
      return { record, exists: false };
    }
    ensure(integrity === record.integrity, 'published version differs; use a new version');
    const tag = JSON.parse(invoke(['view', record.name, 'dist-tags.alpha', '--json', '--registry', registry]));
    ensure(tag === record.version, 'existing version has a different alpha tag; reconcile explicitly');
    return { record, exists: true };
  });
  for (const { record, exists } of states) {
    if (!exists) invoke(['publish', record.file, '--access', 'public', '--tag', 'alpha', '--ignore-scripts', '--provenance', '--registry', registry]);
    report(`${exists ? 'Already published' : 'Published'} ${record.name}@${record.version} (alpha)`);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const { values } = parseArgs({ options: { mode: { type: 'string', default: 'verify' }, packages: { type: 'string' }, output: { type: 'string' }, root: { type: 'string' }, scope: { type: 'string' }, platform: { type: 'string' }, version: { type: 'string' } } });
    if (values.mode === 'pack') {
      ensure(values.packages && values.output, 'pack requires packages and fresh output');
      console.log(JSON.stringify(packRelease(values)));
    } else {
      ensure(['verify', 'publish'].includes(values.mode) && values.root, 'invalid release mode/root');
      const plan = releasePlan(values);
      if (values.mode === 'publish') {
        ensure(process.env.NODE_AUTH_TOKEN, 'NPM_TOKEN is required through NODE_AUTH_TOKEN');
        publishPlan(plan);
      } else console.log(JSON.stringify(plan, null, 2));
    }
  } catch (error) {
    // npm errors can include configuration: never echo raw child diagnostics.
    console.error('MagicVault npm release failed; verify scope, version, complete artifacts and registry state.');
    process.exitCode = 1;
  }
}
