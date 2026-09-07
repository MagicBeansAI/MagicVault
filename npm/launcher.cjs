'use strict';

// No downloads, shell, lifecycle hooks, credentials, daemon startup or stdout
// diagnostics here. npm selects a prebuilt package; Rust owns the application.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn } = require('node:child_process');
const { constants } = require('node:os');

function jsonFile(file) {
  const fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW | fs.constants.O_NONBLOCK);
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > 64 * 1024) throw new Error('invalid_metadata');
    return JSON.parse(fs.readFileSync(fd, 'utf8'));
  } finally { fs.closeSync(fd); }
}

function resolveBinary(command, packageFile = path.join(__dirname, 'package.json'), platform = process.platform, arch = process.arch) {
  if (!['magicvault', 'magicvault-mcp'].includes(command)) throw new Error('invalid_command');
  if (`${platform}-${arch}` !== 'darwin-arm64') throw new Error('unsupported_platform');
  const metadata = jsonFile(packageFile);
  const nativeName = metadata.magicvault?.platforms?.[`${platform}-${arch}`];
  if (typeof nativeName !== 'string' || !/^@[a-z0-9][a-z0-9-]{0,63}\/magicvault-darwin-arm64$/.test(nativeName)
      || metadata.name !== nativeName.replace(/-darwin-arm64$/, '')
      || metadata.optionalDependencies?.[nativeName] !== metadata.version) throw new Error('invalid_package');
  const nativeFile = require.resolve(`${nativeName}/package.json`, { paths: [path.dirname(packageFile)] });
  const native = jsonFile(nativeFile);
  if (native.name !== nativeName || native.version !== metadata.version) throw new Error('version_mismatch');
  const root = fs.realpathSync(path.dirname(nativeFile));
  const manifest = jsonFile(path.join(root, 'bundle.json'));
  if (manifest.format_version !== 1 || manifest.version !== metadata.version || manifest.platform !== `${platform}-${arch}`) throw new Error('invalid_bundle');
  const relative = `bin/${command}`;
  const expected = manifest.files?.[relative];
  const file = path.join(root, relative);
  const stat = fs.lstatSync(file);
  if (fs.realpathSync(file) !== file || !stat.isFile() || !expected?.executable
      || !Number.isSafeInteger(expected.bytes) || expected.bytes < 1 || expected.bytes > 128 * 1024 * 1024
      || stat.size !== expected.bytes) throw new Error('invalid_binary');
  const bytes = fs.readFileSync(file);
  if (bytes.length !== expected.bytes || crypto.createHash('sha256').update(bytes).digest('hex') !== expected.sha256) throw new Error('integrity_failure');
  return file;
}

function launch(command) {
  let executable;
  try { executable = resolveBinary(command); }
  catch {
    // Paths and arguments may contain sensitive user input: never echo them.
    process.stderr.write('MagicVault: compatible prebuilt package unavailable or invalid; reinstall with optional dependencies enabled. macOS Apple Silicon is currently supported.\n');
    process.exitCode = 1;
    return;
  }
  const child = spawn(executable, process.argv.slice(2), { stdio: 'inherit', shell: false });
  const handlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    const forward = () => child.kill(signal);
    handlers.set(signal, forward);
    process.on(signal, forward);
  }
  const cleanup = () => { for (const [signal, handler] of handlers) process.removeListener(signal, handler); };
  child.once('error', () => {
    cleanup();
    process.stderr.write('MagicVault: native executable could not start.\n');
    process.exitCode = 1;
  });
  child.once('exit', (code, signal) => {
    cleanup();
    process.exitCode = code ?? (128 + (constants.signals[signal] ?? 1));
  });
}

module.exports = { resolveBinary, launch };
