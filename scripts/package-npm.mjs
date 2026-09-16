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
export const binaries = ['magicvault', 'magicvault-mcp', 'magicvault-native-host', 'magicvault-prompt'];

export const platforms = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64', 'win32-x64', 'win32-arm64'];
export function standaloneVersion(repo) {
  const versions = ['magicvault', 'magicvault-mcp', 'magicvault-service', 'magicvault-prompt'].map(name =>
    fs.readFileSync(path.join(repo, name, 'Cargo.toml'), 'utf8').match(/^version = "((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))"$/m)?.[1]);
  if (!versions[0] || versions.some(version => version !== versions[0])) throw new Error('standalone package versions must match');
  return versions[0];
}
export function binaryMatches(bytes, platform) {
  const [os, arch] = platform.split('-');
  if (os === 'darwin') return bytes.length >= 32 && bytes.readUInt32LE(0) === 0xfeedfacf && bytes.readUInt32LE(4) === (arch === 'arm64' ? 0x0100000c : 0x01000007);
  if (os === 'linux') return bytes.length >= 64 && bytes.subarray(0, 4).equals(Buffer.from([0x7f, 69, 76, 70])) && bytes[4] === 2 && bytes[5] === 1 && bytes.readUInt16LE(18) === (arch === 'arm64' ? 183 : 62);
  if (bytes.length < 64 || bytes.toString('ascii', 0, 2) !== 'MZ') return false;
  const offset = bytes.readUInt32LE(60);
  return offset >= 64 && offset <= bytes.length - 26 && bytes.readUInt32LE(offset) === 0x00004550 && bytes.readUInt16LE(offset + 4) === (arch === 'arm64' ? 0xaa64 : 0x8664) && bytes.readUInt16LE(offset + 24) === 0x20b;
}

function regular(file, max) {
  if (!fs.lstatSync(file).isFile()) throw new Error('invalid artifact');
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
export function assemble({ repo, binaryDir, output, scope, platform = 'darwin-arm64', registryReadme = false }) {
  if (!platforms.includes(platform)) throw new Error('unsupported platform');
  const [os, arch] = platform.split('-');
  const suffix = os === 'win32' ? '.exe' : '';
  if (typeof scope !== 'string' || scope.trim() !== scope || !/^@[a-z0-9][a-z0-9-]{0,63}$/.test(scope)) throw new Error('an explicitly owned npm scope is required');
  if (registryReadme && scope === '@magicvault-local') throw new Error('select an owned public npm scope for release packages');
  const version = standaloneVersion(repo);
  // Validate every source before creating output, including native architecture.
  const content = new Map();
  for (const name of binaries) {
    const bytes = regular(path.join(binaryDir, name + suffix), 128 * 1024 * 1024);
    if (!binaryMatches(bytes, platform)) throw new Error('binary architecture does not match platform');
    content.set(`bin/${name}${suffix}`, bytes);
  }
  for (const [destination, source] of Object.entries(assets)) content.set(destination, regular(path.join(repo, source), 1024 * 1024));
  fs.mkdirSync(output); // Refuse reuse/overwrite of any existing output.
  const mainName = `${scope}/magicvault`;
  const common = {
    version, license: 'MIT OR Apache-2.0',
    repository: { type: 'git', url: 'git+https://github.com/MagicBeansAI/MagicVault.git' },
    engines: { node: '>=22' }, publishConfig: { access: 'public', tag: 'latest', registry: 'https://registry.npmjs.org/' },
    homepage: 'https://github.com/MagicBeansAI/MagicVault#readme',
    bugs: { url: 'https://github.com/MagicBeansAI/MagicVault/issues' },
    keywords: ['mcp', 'credentials', 'vault', 'agents', 'typescript', 'alpha'],
  };
  const manifest = { format_version: 1, version, platform, files: {} };
  for (const [relative, bytes] of content) {
    const executable = relative.startsWith('bin/');
    write(path.join(output, 'launcher', 'native', platform, relative), bytes, executable ? 0o755 : 0o644);
    manifest.files[relative] = { sha256: crypto.createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length, executable };
  }
  write(path.join(output, 'launcher', 'native', platform, 'bundle.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  const launcherFiles = ['cli.cjs', 'mcp.cjs', 'launcher.cjs', 'sdk.cjs', 'sdk.d.cts'];
  for (const name of launcherFiles) write(path.join(output, 'launcher', name), regular(path.join(repo, 'npm', name), 64 * 1024), ['cli.cjs', 'mcp.cjs'].includes(name) ? 0o755 : 0o644);
  for (const name of ['LICENSE-MIT', 'LICENSE-APACHE']) write(path.join(output, 'launcher', name), content.get(name));
  let readme = regular(path.join(repo, 'npm/README.md'), 64 * 1024).toString('utf8');
  if (registryReadme) {
    const install = `Install the package into your project (Node 22+; npm's **latest** channel):\n\n\x60\x60\x60bash\nnpm install ${mainName}\nnpx magicvault --profile agent setup\n\x60\x60\x60\n\nFor MCP/CLI use outside a project:\n\n\x60\x60\x60bash\nnpm install --global ${mainName}\nmagicvault --profile agent setup\nmagicvault --profile agent doctor\n\x60\x60\x60\n\nThis single package bundles macOS, Linux and Windows binaries (x64/ARM64).\nThe launcher selects your platform automatically. There are no native-package\ndependencies, install scripts or runtime downloads. Platform support remains alpha.\nExplicit setup opens the human approval flow.\n`;
    const section = /<!-- npm-install:start -->[\s\S]*?<!-- npm-install:end -->/;
    if (!section.test(readme)) throw new Error('missing npm README install section');
    readme = readme.replace(section, install);
  }
  readme = readme.replaceAll('@magicvault-local/', `${scope}/`);
  write(path.join(output, 'launcher/README.md'), readme);
  write(path.join(output, 'launcher/package.json'), `${JSON.stringify({ ...common, name: mainName, description: 'Let agents use credentials without seeing them — reference-only credential delivery', bin: { magicvault: 'cli.cjs', 'magicvault-mcp': 'mcp.cjs' }, main: './sdk.cjs', types: './sdk.d.cts', exports: { '.': { types: './sdk.d.cts', default: './sdk.cjs' }, './package.json': './package.json' }, private: true, os: [os], cpu: [arch], files: [...launcherFiles, 'LICENSE-MIT', 'LICENSE-APACHE', 'native'], magicvault: { layout: 'bundled-v1', platforms: { [platform]: `native/${platform}` } } }, null, 2)}\n`);
  return { version, mainName };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try {
    const { values } = parseArgs({ options: { 'binary-dir': { type: 'string' }, output: { type: 'string' }, scope: { type: 'string' }, platform: { type: 'string' }, 'registry-readme': { type: 'boolean', default: false } } });
    if (!values['binary-dir'] || !values.output || !values.scope) throw new Error('missing arguments');
    const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
    console.log(JSON.stringify(assemble({ repo, binaryDir: path.resolve(values['binary-dir']), output: path.resolve(values.output), scope: values.scope, platform: values.platform ?? `${process.platform}-${process.arch}`, registryReadme: values['registry-readme'] })));
  } catch {
    console.error('MagicVault packaging failed: require --binary-dir, fresh --output, explicit --scope and matching --platform release artifacts.');
    process.exitCode = 1;
  }
}
