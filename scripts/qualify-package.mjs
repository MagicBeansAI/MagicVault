#!/usr/bin/env node
// Real, offline npm install and native lifecycle with disposable directories.
// No setup without --install-only, OS services, keychain or publication.
// Browser qualification requires a separate explicit flag and executable.
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { parseArgs } from 'node:util';

const { values } = parseArgs({ options: { packages: { type: 'string' }, work: { type: 'string' }, 'app-parent': { type: 'string' }, 'browser-idle-secs': { type: 'string' }, 'reliability-rounds': { type: 'string', default: '1' }, 'with-performance-tests': { type: 'boolean', default: false }, 'with-rust-tests': { type: 'boolean', default: false }, 'with-browser-tests': { type: 'boolean', default: false }, 'with-process-diagnostics': { type: 'boolean', default: false } } });
const browserIdle = values['browser-idle-secs'];
if (browserIdle !== undefined && (!/^(?:[3-9][0-9]|[12][0-9]{2}|300)$/.test(browserIdle) || !values['with-browser-tests'] || values['reliability-rounds'] !== '1' || process.env.MAGICVAULT_TEST_STARTUP_DIAGNOSTICS !== undefined)) throw new Error('browser resource observations require --with-browser-tests, 30..300 idle seconds, one round and no stack sampling');
if (values['with-process-diagnostics'] && (!values['with-rust-tests'] || process.env.RUSTFLAGS || process.env.CARGO_ENCODED_RUSTFLAGS)) throw new Error('process diagnostics require --with-rust-tests and no ambient Rust flags');
if (!/^(?:[1-9]|1[0-9]|20)$/.test(values['reliability-rounds']) || ((values['reliability-rounds'] !== '1' || values['with-performance-tests']) && !values['with-rust-tests'])) throw new Error('reliability qualification requires --with-rust-tests and 1..20 rounds');
const rounds = Number(values['reliability-rounds']);
if (values['with-browser-tests'] && (!values['with-rust-tests'] || !process.env.MAGICVAULT_CHROME || !path.isAbsolute(process.env.MAGICVAULT_CHROME) || !fs.statSync(process.env.MAGICVAULT_CHROME).isFile())) throw new Error('browser qualification requires --with-rust-tests and an explicit absolute MAGICVAULT_CHROME executable');
// A native host on an external volume can block in dyld while macOS requests
// removable-volume access, before our synthetic consent provider can run.
// Require a deliberate app location for browser qualification; use an internal
// parent for the non-interactive lane. Builds/packages/profiles can stay on SSD.
if (values['with-browser-tests'] && !values['app-parent']) throw new Error('browser qualification requires explicit --app-parent (normally /private/tmp); external application volumes can request native OS permission');
if (values['app-parent'] && (!path.isAbsolute(values['app-parent']) || !fs.statSync(values['app-parent']).isDirectory())) throw new Error('app-parent must be an existing absolute directory');
if (!values.packages || !values.work || process.platform !== 'darwin' || process.arch !== 'arm64') throw new Error('require --packages, fresh --work, and macOS arm64');
const packages = fs.realpathSync(values.packages);
const work = path.resolve(values.work);
fs.mkdirSync(work); // Fresh, caller-selected qualification directory only.
const nodeDir = path.join(work, 'node-only'); fs.mkdirSync(nodeDir);
fs.symlinkSync(process.execPath, path.join(nodeDir, 'node'));
const npm = process.env.PATH.split(path.delimiter).map(p => path.join(p, 'npm')).find(p => fs.existsSync(p));
if (!npm) throw new Error('npm required by qualification driver');
const npmCli = fs.realpathSync(npm);
const home = path.join(work, 'home'); fs.mkdirSync(home);
const config = path.join(work, 'empty.npmrc'); fs.writeFileSync(config, '', { flag: 'wx' });
const globalConfig = path.join(work, 'global.npmrc'); fs.writeFileSync(globalConfig, '', { flag: 'wx' });
const env = { ...process.env, HOME: home, PATH: `${nodeDir}:/usr/bin:/bin:/usr/sbin:/sbin`, npm_config_cache: path.join(work, 'npm-cache'), npm_config_userconfig: config, npm_config_globalconfig: globalConfig };
const run = (exe, args, options = {}) => execFileSync(exe, args, { env, encoding: 'utf8', timeout: 60_000, stdio: ['ignore', 'pipe', 'pipe'], ...options });
const npmRun = (args, cwd) => run(process.execPath, [npmCli, ...args], { cwd });
function pack(directory) {
  const result = JSON.parse(npmRun(['pack', '--ignore-scripts', '--offline', '--json', '--pack-destination', work], directory));
  assert.equal(result.length, 1);
  assert(!result[0].files.some(f => /(?:Cargo\.lock|\.env|\.p12|\.p8|client-.*\.json|instance\.json)$/.test(f.path)));
  return path.join(work, result[0].filename);
}
const nativeTarball = pack(path.join(packages, 'native'));
const launcherTarball = pack(path.join(packages, 'launcher'));
const prefix = path.join(work, 'client'); fs.mkdirSync(prefix);
npmRun(['install', '--prefix', prefix, '--offline', '--ignore-scripts', '--no-audit', '--no-fund', launcherTarball, nativeTarball], prefix);
assert.deepEqual(fs.readdirSync(home), []); // npm install did not initialize anything.
const cli = path.join(prefix, 'node_modules/.bin/magicvault');
const mcp = path.join(prefix, 'node_modules/.bin/magicvault-mcp');
const version = JSON.parse(fs.readFileSync(path.join(packages, 'launcher/package.json'))).version;
assert.equal(run(cli, ['--version']).trim(), `magicvault ${version}`);
assert.equal(run(mcp, ['--version']).trim(), `magicvault-mcp ${version}`);
const vault = path.join(work, 'vault');
const appArtifacts = values['app-parent'] ? fs.mkdtempSync(path.join(fs.realpathSync(values['app-parent']), 'mv-package-app-')) : work;
const app = path.join(appArtifacts, 'app');
const invoke = args => JSON.parse(run(cli, ['--root', vault, '--app-dir', app, ...args]));
assert.equal(invoke(['doctor']).installation.installed, false);
assert(!fs.existsSync(app)); assert(!fs.existsSync(vault));
const setup = invoke(['--profile', 'agent', 'setup', '--install-only']);
assert.equal(setup.service_started, false); assert(!fs.existsSync(vault));
assert.equal(invoke(['doctor']).installation.integrity_verified, true);
assert.equal(invoke(['upgrade']).upgraded, true);
assert.equal(run(setup.mcpServers.magicvault.command, ['--version']).trim(), `magicvault-mcp ${version}`);

if (values['with-rust-tests']) {
  // Rust is needed by the test DRIVER, never the installed clients. Reuse real
  // IPC/effect tests with their in-memory keys and synthetic human interaction.
  const testEnv = { ...process.env, MAGICVAULT_TEST_CLI: cli, MAGICVAULT_TEST_MCP: mcp, MAGICVAULT_TEST_NATIVE_HOST: path.join(app, 'current/bin/magicvault-native-host'), MAGICVAULT_TEST_EXTENSION: setup.extension_directory };
  // An ambient test variable must not silently add minutes to normal CI.
  delete testEnv.MAGICVAULT_BROWSER_RESOURCE_IDLE_SECS;
  if (browserIdle !== undefined) testEnv.MAGICVAULT_BROWSER_RESOURCE_IDLE_SECS = browserIdle;
  // Only the synthetic test DRIVER is instrumented; installed clients remain
  // the reviewed package bytes. Never share these build outputs with packaging.
  const diagnosticEnv = { ...testEnv, CARGO_TARGET_DIR: path.join(work, 'diagnostic-builds'), CARGO_ENCODED_RUSTFLAGS: '--cfg\x1fmagicvault_test_diagnostics' };
  if (values['with-process-diagnostics']) {
    const diagnosticRun = args => execFileSync('cargo', ['test', '--locked', ...args], { env: diagnosticEnv, stdio: 'inherit', timeout: 300_000 });
    diagnosticRun(['-p', 'magicvault-effect', '--lib', 'test_diagnostics::']);
    diagnosticRun(['-p', 'magicvault-effect', '--test', 'delivery_diagnostics']);
    diagnosticRun(['-p', 'magicvault', '--test', 'delivery_cli', '--no-run']);
    let releaseRefused = false;
    try {
      execFileSync('cargo', ['check', '--locked', '--release', '-p', 'magicvault-effect', '--lib'], { env: diagnosticEnv, stdio: 'pipe', timeout: 300_000 });
    } catch (error) {
      releaseRefused = Number.isInteger(error.status) && error.status !== 0 && String(error.stderr).includes('magicvault_test_diagnostics is forbidden in release builds');
    }
    assert(releaseRefused, 'instrumented release must fail with the explicit isolation guard');
    console.log(JSON.stringify({ diagnostic_release_refused: true }));
  }
  for (let round = 1; round <= rounds; round++) {
    // These are independent trials, never retries after failure. execFileSync
    // throws at the first failing lane; no success summary or uninstall follows.
    console.log(JSON.stringify({ qualification_round: round, rounds }));
    if (values['with-process-diagnostics']) {
      execFileSync('cargo', ['test', '--locked', '-p', 'magicvault', '--test', 'cli_flow', '--test', 'native_host'], { env: testEnv, stdio: 'inherit', timeout: 120_000 });
      execFileSync('cargo', ['test', '--locked', '-p', 'magicvault', '--test', 'delivery_cli', '--', '--nocapture'], { env: diagnosticEnv, stdio: 'inherit', timeout: 120_000 });
    } else {
      execFileSync('cargo', ['test', '--locked', '-p', 'magicvault', '--test', 'cli_flow', '--test', 'delivery_cli', '--test', 'native_host'], { env: testEnv, stdio: 'inherit', timeout: 120_000 });
    }
    execFileSync('cargo', ['test', '--locked', '-p', 'magicvault-mcp', '--test', 'end_to_end'], { env: testEnv, stdio: 'inherit', timeout: 120_000 });
    if (values['with-browser-tests']) {
      // Real Chrome dispatch, isolated user-data roots/host definitions and
      // synthetic human/key providers. Not native permission/keychain acceptance.
      execFileSync('cargo', ['test', '--locked', '--release', '-p', 'magicvault-mcp', '--test', 'extension_native', '--', '--ignored', '--nocapture', '--test-threads=1'], { env: testEnv, stdio: 'inherit', timeout: 120_000 + Number(browserIdle || 0) * 1000 });
    }
  }
  if (values['with-performance-tests']) {
    execFileSync('cargo', ['test', '--locked', '--release', '-p', 'magicvault', '--test', 'delivery_cli', '--', '--ignored', '--nocapture', '--test-threads=1'], { env: testEnv, stdio: 'inherit', timeout: 180_000 });
  }
}

// Remove the npm packages using npm itself, then prove the stable native paths
// continue working. This is the ephemeral-cache/upgrade regression boundary.
const main = JSON.parse(fs.readFileSync(path.join(packages, 'launcher/package.json')));
const native = JSON.parse(fs.readFileSync(path.join(packages, 'native/package.json')));
npmRun(['uninstall', '--prefix', prefix, '--offline', '--ignore-scripts', '--no-audit', '--no-fund', main.name, native.name], prefix);
const stableCli = path.join(app, 'current/bin/magicvault');
assert.equal(run(stableCli, ['--version']).trim(), `magicvault ${version}`);
const retired = JSON.parse(run(stableCli, ['--root', vault, '--app-dir', app, 'uninstall']));
assert.equal(retired.vault_preserved, true); assert.equal(retired.keychain_preserved, true);
assert(!fs.existsSync(app)); assert(fs.existsSync(retired.application_archive));
assert(!fs.existsSync(vault)); assert.deepEqual(fs.readdirSync(home), []);
console.log(JSON.stringify({ passed: true, version, npm_install: 'offline local tarballs', client_path: 'Node and system utilities only; no Rust', live_custody_or_services_touched: false, rust_fixture_tests: values['with-rust-tests'], isolated_browser_tests: values['with-browser-tests'], browser_idle_seconds: Number(browserIdle || 0), process_diagnostics: values['with-process-diagnostics'], reliability_rounds: rounds, performance_tests: values['with-performance-tests'], artifacts: work, application_artifacts: appArtifacts }));
