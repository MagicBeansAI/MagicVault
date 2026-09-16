#!/usr/bin/env node
// Build private platform candidates, combine them, and publish one self-contained package.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { platforms, assets, binaries, binaryMatches, standaloneVersion } from './package-npm.mjs';

const registry = 'https://registry.npmjs.org/';
const versionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const launcherFiles = ['package.json', 'README.md', 'cli.cjs', 'mcp.cjs', 'launcher.cjs', 'sdk.cjs', 'sdk.d.cts', 'LICENSE-MIT', 'LICENSE-APACHE'];
const digest = bytes => `sha512-${crypto.createHash('sha512').update(bytes).digest('base64')}`;
const sha256 = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const sleep = ms => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
function ensure(ok, message) { if (!ok) throw new Error(message); }
function identity(scope, version) {
  ensure(typeof scope === 'string' && scope.trim() === scope && /^@[a-z0-9][a-z0-9-]{0,63}$/.test(scope) && scope !== '@magicvault-local', 'select an owned public npm scope');
  ensure(typeof version === 'string' && version.trim() === version && versionPattern.test(version), 'invalid release version');
}
export function releaseContext({ scope, version, event, ref }) {
  identity(scope, version);
  ensure(['push', 'workflow_dispatch'].includes(event), 'unsupported release event');
  const isTag = ref?.startsWith('refs/tags/');
  if (isTag) ensure(ref === `refs/tags/v${version}`, 'release tag must exactly match standalone versions');
  else ensure(ref === 'refs/heads/main', 'prepare releases from main or the matching version tag');
  return { scope, version, tag: 'latest', publish: event === 'push' && isTag };
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
export function npm(args, options = {}) {
  return execFileSync(process.execPath, [npmCli(), ...args], {
    encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], timeout: 120000, maxBuffer: 1024 * 1024, ...options,
  });
}
function checkMetadata(meta, scope, version, selected, candidate) {
  ensure(meta.name === `${scope}/magicvault` && meta.version === version, 'package identity mismatch');
  ensure(!['scripts', 'dependencies', 'optionalDependencies', 'devDependencies', 'peerDependencies', 'bundledDependencies', 'bundleDependencies'].some(k => Object.hasOwn(meta, k)), 'unexpected hooks or package dependencies');
  ensure(meta.publishConfig?.access === 'public' && meta.publishConfig?.tag === 'latest' && meta.publishConfig?.registry === registry
    && Object.keys(meta.publishConfig).sort().join(',') === 'access,registry,tag', 'unsafe publication metadata');
  ensure(meta.repository?.url === 'git+https://github.com/MagicBeansAI/MagicVault.git', 'wrong source repository');
  ensure(candidate ? meta.private === true : !Object.hasOwn(meta, 'private'), 'candidate publication boundary mismatch');
  ensure(meta.magicvault?.layout === 'bundled-v1'
    && JSON.stringify(Object.keys(meta.magicvault.platforms || {}).sort()) === JSON.stringify([...selected].sort())
    && selected.every(p => meta.magicvault.platforms[p] === `native/${p}`), 'wrong bundled platform map');
  const os = [...new Set(selected.map(p => p.split('-')[0]))];
  const cpu = [...new Set(selected.map(p => p.split('-')[1]))];
  ensure(JSON.stringify(meta.os) === JSON.stringify(os) && JSON.stringify(meta.cpu) === JSON.stringify(cpu), 'platform constraints mismatch');
  ensure(meta.bin?.magicvault === 'cli.cjs' && meta.bin?.['magicvault-mcp'] === 'mcp.cjs'
    && Object.keys(meta.bin).length === 2 && meta.main === './sdk.cjs' && meta.types === './sdk.d.cts', 'wrong public entry points');
}
function nativeFiles(platform) {
  const suffix = platform.startsWith('win32-') ? '.exe' : '';
  return ['bundle.json', ...Object.keys(assets), ...binaries.map(b => `bin/${b}${suffix}`)];
}
function archiveFile(file, relative, maxBuffer = 65536) {
  // Read to stdout only. Never extract archive-supplied paths or links to disk.
  return execFileSync('tar', ['-xOf', file, `package/${relative}`], { timeout: 120000, maxBuffer, stdio: ['ignore', 'pipe', 'pipe'] });
}
function archiveMetadata(file, selected) {
  const listing = execFileSync('tar', ['-tzf', file], { encoding: 'utf8', timeout: 120000, maxBuffer: 65536, stdio: ['ignore', 'pipe', 'pipe'] }).trim().split('\n');
  const expected = [...launcherFiles, ...selected.flatMap(p => nativeFiles(p).map(f => `native/${p}/${f}`))].map(p => `package/${p}`).sort();
  ensure(JSON.stringify(listing.sort()) === JSON.stringify(expected), 'unexpected files in release tarball');
  return JSON.parse(archiveFile(file, 'package.json'));
}
function pack(directory, output, scope, version, selected, candidate) {
  identity(scope, version);
  checkMetadata(json(path.join(directory, 'package.json')), scope, version, selected, candidate);
  fs.mkdirSync(output);
  const [packed] = JSON.parse(npm(['pack', path.resolve(directory), '--json', '--ignore-scripts', '--offline', '--pack-destination', path.resolve(output)]));
  const filename = `${scope.slice(1)}-magicvault-${version}.tgz`;
  ensure(packed.name === `${scope}/magicvault` && packed.version === version && packed.filename === filename, 'unexpected packed identity');
  const integrity = digest(read(path.join(output, filename), 512 * 1024 * 1024));
  ensure(integrity === packed.integrity, 'packed integrity mismatch');
  const record = { name: packed.name, version, filename, integrity, kind: candidate ? 'candidate' : 'universal' };
  fs.writeFileSync(path.join(output, 'release.json'), JSON.stringify({ schema: 2, scope, version, platforms: selected, entries: [record] }, null, 2) + '\n', { flag: 'wx' });
  return [record];
}
export function packRelease({ packages, output, scope, platform }) {
  ensure(platforms.includes(platform), 'unsupported release platform');
  return pack(path.join(packages, 'launcher'), output, scope, json(path.join(packages, 'launcher/package.json')).version, [platform], true);
}
function recordAt(root, scope, version, selected, candidate) {
  const manifest = json(path.join(root, 'release.json'));
  ensure(manifest.schema === 2 && manifest.scope === scope && manifest.version === version
    && JSON.stringify(manifest.platforms) === JSON.stringify(selected), 'release set identity mismatch');
  ensure(Array.isArray(manifest.entries) && manifest.entries.length === 1, 'release must contain exactly one package');
  const record = manifest.entries[0];
  ensure(record.kind === (candidate ? 'candidate' : 'universal') && record.name === `${scope}/magicvault`
    && record.version === version && record.filename === `${scope.slice(1)}-magicvault-${version}.tgz`, 'invalid release record');
  const file = path.join(root, record.filename);
  ensure(digest(read(file, 512 * 1024 * 1024)) === record.integrity, 'release tarball changed');
  const metadata = archiveMetadata(file, selected);
  checkMetadata(metadata, scope, version, selected, candidate);
  return { ...record, file, metadata };
}
export function assembleRelease({ root, output, scope, version }) {
  identity(scope, version);
  // Verify every candidate before creating the final output. Candidate packages are private.
  const candidates = platforms.map(p => recordAt(path.join(root, `MagicVault-npm-${p}`), scope, version, [p], true));
  const common = new Map(launcherFiles.filter(p => p !== 'package.json').map(p => [p, archiveFile(candidates[0].file, p)]));
  const content = new Map(common);
  for (let i = 0; i < platforms.length; i++) {
    const platform = platforms[i], candidate = candidates[i];
    for (const [name, bytes] of common) ensure(archiveFile(candidate.file, name).equals(bytes), 'platform candidates disagree on launcher or README');
    const prefix = `native/${platform}`;
    const bundle = JSON.parse(archiveFile(candidate.file, `${prefix}/bundle.json`));
    ensure(bundle.format_version === 1 && bundle.version === version && bundle.platform === platform, 'native bundle identity mismatch');
    const names = nativeFiles(platform).filter(f => f !== 'bundle.json');
    ensure(JSON.stringify(Object.keys(bundle.files).sort()) === JSON.stringify([...names].sort()), 'native bundle file set mismatch');
    for (const name of names) {
      const executable = name.startsWith('bin/');
      const bytes = archiveFile(candidate.file, `${prefix}/${name}`, executable ? 128 * 1024 * 1024 : 1024 * 1024);
      const expected = bundle.files[name];
      ensure(expected?.bytes === bytes.length && expected.sha256 === sha256(bytes) && expected.executable === executable, 'native bundle integrity mismatch');
      if (executable) ensure(binaryMatches(bytes, platform), 'native architecture mismatch');
      else if (Object.hasOwn(assets, name)) ensure(bytes.equals(read(path.resolve(import.meta.dirname, '..', assets[name]), 1024 * 1024)), 'native asset differs from release source');
      content.set(`${prefix}/${name}`, bytes);
    }
    content.set(`${prefix}/bundle.json`, Buffer.from(JSON.stringify(bundle, null, 2) + '\n'));
  }
  const metadata = { ...candidates[0].metadata };
  delete metadata.private;
  metadata.os = [...new Set(platforms.map(p => p.split('-')[0]))];
  metadata.cpu = [...new Set(platforms.map(p => p.split('-')[1]))];
  metadata.magicvault = { layout: 'bundled-v1', platforms: Object.fromEntries(platforms.map(p => [p, `native/${p}`])) };
  content.set('package.json', Buffer.from(JSON.stringify(metadata, null, 2) + '\n'));
  fs.mkdirSync(output);
  const directory = path.join(output, 'package');
  for (const [relative, bytes] of content) {
    const file = path.join(directory, relative);
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, bytes, { flag: 'wx', mode: relative.includes('/bin/') || ['cli.cjs', 'mcp.cjs'].includes(relative) ? 0o755 : 0o644 });
  }
  return pack(directory, path.join(output, 'tarballs'), scope, version, platforms, false);
}
export function releasePlan({ root, scope, version }) {
  identity(scope, version);
  const { metadata, ...record } = recordAt(root, scope, version, platforms, false);
  return [record];
}
function lookup(invoke, name, field) {
  try {
    const raw = invoke(['view', name, ...(field ? [field] : []), '--json', '--prefer-online', '--registry', registry]);
    return { missing: false, value: raw.trim() ? JSON.parse(raw) : undefined };
  } catch (error) {
    let code;
    try { code = JSON.parse(String(error.stdout)).error?.code; } catch { /* closed below */ }
    ensure(code === 'E404', 'registry lookup failed');
    return { missing: true };
  }
}
function newer(a, b) {
  ensure(typeof a === 'string' && a.trim() === a && versionPattern.test(a), 'invalid latest version; reconcile explicitly');
  const first = a.split('.').map(BigInt), second = b.split('.').map(BigInt);
  for (let i = 0; i < 3; i++) if (first[i] !== second[i]) return first[i] > second[i];
  return false;
}
function verifyEventually(check, wait, delays = [1000, 2000, 4000, 8000, 16000]) {
  // Only repeat read-only checks. Uploads and deprecations are never retried here.
  for (let attempt = 0; attempt <= delays.length; attempt++) {
    if (check()) return;
    if (attempt < delays.length) wait(delays[attempt]);
  }
  throw new Error('post-publication verification failed; inspect registry state');
}
export function publishPlan(plan, invoke = npm, report = console.log, wait = sleep) {
  ensure(plan.length === 1 && /^@[a-z0-9][a-z0-9-]{0,63}\/magicvault$/.test(plan[0].name), 'publish only the single main package');
  const record = plan[0];
  const artifact = lookup(invoke, `${record.name}@${record.version}`, 'dist.integrity');
  const latest = lookup(invoke, record.name, 'dist-tags.latest');
  if (!artifact.missing) ensure(artifact.value === record.integrity, 'published version differs; use a new version');
  ensure(artifact.missing || !latest.missing, 'inconsistent registry response');
  if (latest.value !== undefined) ensure(!newer(latest.value, record.version), 'refusing to move latest backwards');
  if (artifact.missing) invoke(['publish', record.file, '--access', 'public', '--tag', 'latest', '--ignore-scripts', '--provenance', '--registry', registry]);
  else if (latest.value !== record.version) invoke(['dist-tag', 'add', `${record.name}@${record.version}`, 'latest', '--registry', registry]);
  report(`${artifact.missing ? 'Published' : 'Already published'} ${record.name}@${record.version} (latest)`);
  // npm packuments are publicly cached for up to 300 seconds. Allow a complete
  // cache lifetime after upload, without ever repeating the upload itself.
  verifyEventually(() => {
    const integrity = lookup(invoke, `${record.name}@${record.version}`, 'dist.integrity');
    ensure(integrity.missing || integrity.value === record.integrity, 'published bytes differ');
    return !integrity.missing && lookup(invoke, record.name, 'dist-tags.latest').value === record.version;
  }, wait, [1000, 2000, 4000, 8000, 16000, 30000, 60000, 60000, 60000, 60000]);
}
export function retireLegacy({ scope, version }, invoke = npm, report = console.log, wait = sleep) {
  ensure(scope === '@magicbeansai' && version === '0.9.1', 'legacy migration is scoped to the 0.9.1 release');
  const name = `${scope}/magicvault`;
  ensure(lookup(invoke, name, 'dist-tags.latest').value === version, 'replacement must be latest before migration');
  const replacement = lookup(invoke, `${name}@${version}`, '').value;
  checkMetadata(replacement, scope, version, platforms, false);
  const message = 'Retired compatibility package for MagicVault 0.9.0. Install @magicbeansai/magicvault@latest; version 0.9.1+ bundles every platform in one package.';
  const legacy = platforms.map(p => `${name}-${p}`);
  // Validate the entire known legacy set before any deprecation. Never delete bytes.
  for (const target of legacy) {
    ensure(JSON.stringify(lookup(invoke, target, 'versions').value) === JSON.stringify(['0.9.0']), 'unexpected legacy versions; review migration');
  }
  ensure(lookup(invoke, `${name}@0.9.0`, 'version').value === '0.9.0', 'legacy main version missing');
  for (const target of [...legacy, `${name}@0.9.0`]) {
    const spec = target === `${name}@0.9.0` ? target : `${target}@0.9.0`;
    if (lookup(invoke, spec, 'deprecated').value !== message) invoke(['deprecate', target, message, '--registry', registry]);
    verifyEventually(() => lookup(invoke, spec, 'deprecated').value === message, wait);
    report(`Retired ${target}; retained for existing 0.9.0 installs`);
  }
}
export function removeLegacy({ scope, version }, invoke = npm, report = console.log, wait = sleep) {
  ensure(scope === '@magicbeansai' && version === '0.9.2', 'legacy removal is scoped to the 0.9.2 release');
  const name = `${scope}/magicvault`;
  const replacement = lookup(invoke, `${name}@${version}`, '').value;
  checkMetadata(replacement, scope, version, platforms, false);
  ensure(lookup(invoke, name, 'dist-tags.latest').value === version, 'replacement must be latest before removal');
  ensure(typeof replacement.dist?.integrity === 'string' && replacement.dist.integrity.startsWith('sha512-'), 'replacement integrity missing');
  const legacy = platforms.map(p => `${name}-${p}`);
  const oldMain = lookup(invoke, `${name}@0.9.0`, '');
  if (!oldMain.missing) {
    const meta = oldMain.value;
    ensure(meta?.name === name && meta.version === '0.9.0' && Boolean(meta.deprecated)
      && JSON.stringify(Object.entries(meta.optionalDependencies || {}).sort())
        === JSON.stringify(legacy.map(p => [p, '0.9.0']).sort()), 'unexpected legacy main dependency graph');
  }
  const previous = lookup(invoke, `${name}@0.9.1`, 'dist.integrity');
  ensure(typeof previous.value === 'string' && previous.value.startsWith('sha512-'), 'previous self-contained version must remain available');
  // Preflight the entire fixed set. An exact version spec prevents deleting any
  // concurrently added version. Only the obsolete main 0.9.0 can be removed;
  // npm otherwise refuses its six dependencies. Never unpublish the main name.
  const remaining = legacy.filter(target => {
    const versions = lookup(invoke, target, 'versions');
    if (versions.missing) return false;
    ensure(JSON.stringify(versions.value) === JSON.stringify(['0.9.0']), 'unexpected legacy versions; review removal');
    ensure(Boolean(lookup(invoke, `${target}@0.9.0`, 'deprecated').value), 'legacy package must already be deprecated');
    return true;
  });
  for (const target of [...(oldMain.missing ? [] : [name]), ...remaining]) {
    report(`Removing ${target}@0.9.0`);
    try {
      invoke(['unpublish', `${target}@0.9.0`, '--force', '--ignore-scripts', '--json', '--registry', registry]);
    } catch (error) {
      let code = 'UNKNOWN', dependents = false;
      try {
        const diagnostic = JSON.parse(String(error.stdout)).error;
        if (/^E[A-Z0-9_]+$/.test(diagnostic?.code)) code = diagnostic.code;
        dependents = /depend/i.test(String(diagnostic?.summary) + String(diagnostic?.detail));
      } catch { /* Never expose raw npm output or authentication configuration. */ }
      report(`Removal stopped for ${target}: ${code}${dependents ? ' (npm reports dependent packages)' : ''}. No write was retried.`);
      throw new Error('legacy removal failed; inspect registry policy and authentication');
    }
    verifyEventually(() => lookup(invoke, `${target}@0.9.0`, 'version').missing, wait,
      [1000, 2000, 4000, 8000, 16000, 30000, 60000, 60000, 60000, 60000]);
    report(`Removed ${target}@0.9.0`);
  }
  ensure(lookup(invoke, `${name}@${version}`, 'dist.integrity').value === replacement.dist.integrity
    && lookup(invoke, name, 'dist-tags.latest').value === version, 'replacement changed during removal');
  ensure(lookup(invoke, `${name}@0.9.1`, 'dist.integrity').value === previous.value, 'previous self-contained version changed during removal');
  report(`Verified ${name}@${version} remains latest with unchanged bytes`);
}
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const { values } = parseArgs({ options: { mode: { type: 'string', default: 'verify' }, packages: { type: 'string' }, output: { type: 'string' }, root: { type: 'string' }, scope: { type: 'string' }, platform: { type: 'string' }, version: { type: 'string' } } });
    if (values.mode === 'context') {
      const context = releaseContext({ scope: values.scope, version: standaloneVersion(path.resolve(import.meta.dirname, '..')), event: process.env.GITHUB_EVENT_NAME, ref: process.env.GITHUB_REF });
      if (process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, Object.entries(context).map(([key, value]) => `${key}=${value}\n`).join(''));
      console.log(JSON.stringify(context));
    } else if (values.mode === 'pack') {
      ensure(values.packages && values.output, 'pack requires packages and fresh output');
      console.log(JSON.stringify(packRelease(values)));
    } else if (values.mode === 'assemble') {
      ensure(values.root && values.output, 'assemble requires candidates and fresh output');
      console.log(JSON.stringify(assembleRelease(values)));
    } else if (values.mode === 'retire-legacy') {
      ensure(process.env.NODE_AUTH_TOKEN, 'NPM_TOKEN is required through NODE_AUTH_TOKEN');
      retireLegacy(values);
    } else if (values.mode === 'remove-legacy') {
      ensure(process.env.NODE_AUTH_TOKEN, 'NPM_TOKEN is required through NODE_AUTH_TOKEN');
      removeLegacy(values);
    } else {
      ensure(['verify', 'publish'].includes(values.mode) && values.root, 'invalid release mode/root');
      const plan = releasePlan(values);
      if (values.mode === 'publish') {
        ensure(process.env.NODE_AUTH_TOKEN, 'NPM_TOKEN is required through NODE_AUTH_TOKEN');
        publishPlan(plan);
      } else console.log(JSON.stringify(plan, null, 2));
    }
  } catch {
    // npm diagnostics can contain configuration. Never echo raw child output.
    console.error('MagicVault npm release failed; verify scope, version, artifacts and registry state.');
    process.exitCode = 1;
  }
}
