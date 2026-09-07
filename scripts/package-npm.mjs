#!/usr/bin/env node
// Explicit allowlist, local output only. This program never publishes or signs.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export const assets = {
  'extension/manifest.json': 'extension/manifest.json',
  'extension/worker.js': 'extension/worker.js',
  'extension/options.html': 'extension/options.html',
  'extension/options.js': 'extension/options.js',
  'extension/options.css': 'extension/options.css',
  'extension/fill.js': 'magicvault-effect/src/fill.js',
  'examples/secure-fill.json': 'examples/secure-fill.json',
  'examples/http-profile.json': 'examples/http-profile.json',
  'examples/process-profile.json': 'examples/process-profile.json',
  'LICENSE-MIT': 'LICENSE-MIT',
  'LICENSE-APACHE': 'LICENSE-APACHE',
};
export const binaries = ['magicvault', 'magicvault-mcp', 'magicvault-native-host'];

function regular(file, max) {
  const fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > max) throw new Error('invalid artifact');
    return fs.readFileSync(fd);
  } finally { fs.closeSync(fd); }
}
function write(file, bytes, mode = 0o644) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, bytes, { mode, flag: 'wx' });
}
export function assemble({ repo, binaryDir, output, scope }) {
  if (!/^@[a-z0-9][a-z0-9-]{0,63}$/.test(scope)) throw new Error('an explicitly owned npm scope is required');
  const version = fs.readFileSync(path.join(repo, 'magicvault/Cargo.toml'), 'utf8').match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
  const mcpVersion = fs.readFileSync(path.join(repo, 'magicvault-mcp/Cargo.toml'), 'utf8').match(/^version = "(\d+\.\d+\.\d+)"$/m)?.[1];
  if (!version || version !== mcpVersion) throw new Error('native package versions must match');
  // Validate every source before creating output, including Mach-O architecture.
  const content = new Map();
  for (const name of binaries) {
    const bytes = regular(path.join(binaryDir, name), 128 * 1024 * 1024);
    if (bytes.length < 32 || bytes.readUInt32LE(0) !== 0xfeedfacf || bytes.readUInt32LE(4) !== 0x0100000c) throw new Error('expected an arm64 Mach-O executable');
    content.set(`bin/${name}`, bytes);
  }
  for (const [destination, source] of Object.entries(assets)) content.set(destination, regular(path.join(repo, source), 1024 * 1024));
  fs.mkdirSync(output); // Refuse reuse/overwrite of any existing output.
  const mainName = `${scope}/magicvault`;
  const nativeName = `${scope}/magicvault-darwin-arm64`;
  const common = {
    version, license: 'MIT OR Apache-2.0',
    repository: { type: 'git', url: 'git+https://github.com/MagicBeansAI/MagicVault.git' },
    engines: { node: '>=22' }, publishConfig: { access: 'public' },
  };
  const manifest = { format_version: 1, version, platform: 'darwin-arm64', files: {} };
  for (const [relative, bytes] of content) {
    const executable = relative.startsWith('bin/');
    write(path.join(output, 'native', relative), bytes, executable ? 0o755 : 0o644);
    manifest.files[relative] = { sha256: crypto.createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length, executable };
  }
  write(path.join(output, 'native/bundle.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  write(path.join(output, 'native/package.json'), `${JSON.stringify({ ...common, name: nativeName, description: 'Prebuilt MagicVault executables and browser extension for macOS Apple Silicon', os: ['darwin'], cpu: ['arm64'], files: ['bin', 'extension', 'examples', 'bundle.json', 'LICENSE-MIT', 'LICENSE-APACHE'] }, null, 2)}\n`);
  for (const name of ['cli.cjs', 'mcp.cjs', 'launcher.cjs']) write(path.join(output, 'launcher', name), regular(path.join(repo, 'npm', name), 64 * 1024), name === 'launcher.cjs' ? 0o644 : 0o755);
  for (const name of ['LICENSE-MIT', 'LICENSE-APACHE']) write(path.join(output, 'launcher', name), content.get(name));
  write(path.join(output, 'launcher/README.md'), regular(path.join(repo, 'npm/README.md'), 64 * 1024));
  write(path.join(output, 'launcher/package.json'), `${JSON.stringify({ ...common, name: mainName, description: 'Keep secrete away from Agents — reference-only credential delivery', bin: { magicvault: 'cli.cjs', 'magicvault-mcp': 'mcp.cjs' }, files: ['cli.cjs', 'mcp.cjs', 'launcher.cjs', 'LICENSE-MIT', 'LICENSE-APACHE'], optionalDependencies: { [nativeName]: version }, magicvault: { platforms: { 'darwin-arm64': nativeName } } }, null, 2)}\n`);
  return { version, mainName, nativeName };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const { values } = parseArgs({ options: { 'binary-dir': { type: 'string' }, output: { type: 'string' }, scope: { type: 'string' } } });
    if (!values['binary-dir'] || !values.output || !values.scope) throw new Error('missing arguments');
    const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    console.log(JSON.stringify(assemble({ repo, binaryDir: path.resolve(values['binary-dir']), output: path.resolve(values.output), scope: values.scope })));
  } catch {
    console.error('MagicVault packaging failed: require --binary-dir, fresh --output, explicit --scope and matching arm64 release artifacts.');
    process.exitCode = 1;
  }
}
