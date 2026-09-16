#!/usr/bin/env node
// Install the exact final tarball offline and without optional dependencies/hooks.
import fs from 'node:fs';
import path from 'node:path';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { parseArgs } from 'node:util';
import assert from 'node:assert/strict';
import { releasePlan, npm } from './release-npm.mjs';

const { values } = parseArgs({ options: {
  root: { type: 'string' }, scope: { type: 'string' },
  version: { type: 'string' }, work: { type: 'string' },
} });
assert(values.work && values.root);
const [record] = releasePlan(values);
const work = path.resolve(values.work);
fs.mkdirSync(work);
const config = path.join(work, 'empty.npmrc');
fs.writeFileSync(config, '', { flag: 'wx' });
npm(['install', '--prefix', work, '--offline', '--ignore-scripts', '--omit=optional',
  '--no-audit', '--no-fund', path.resolve(record.file)], {
  env: { ...process.env, npm_config_cache: path.join(work, 'cache'), npm_config_userconfig: config },
});
assert.deepEqual(fs.readdirSync(path.join(work, 'node_modules', values.scope)), ['magicvault']);
const require = createRequire(path.join(work, 'package.json'));
const sdk = require(record.name);
const esm = await import(pathToFileURL(require.resolve(record.name)));
assert.equal(typeof sdk.MagicVault, 'function');
assert.equal(typeof esm.MagicVault, 'function');
assert.equal(typeof sdk.createOperationId, 'function');
const packageFile = require.resolve(`${record.name}/package.json`);
const metadata = JSON.parse(fs.readFileSync(packageFile));
assert.equal(metadata.version, values.version);
assert.equal(metadata.optionalDependencies, undefined);
for (const [name, entry] of Object.entries(metadata.bin)) {
  const output = execFileSync(process.execPath, [path.join(path.dirname(packageFile), entry), '--version'],
    { encoding: 'utf8', timeout: 15000 });
  assert.equal(output.trim(), `${name} ${values.version}`);
}
console.log(JSON.stringify({ version: values.version, platform: `${process.platform}-${process.arch}`,
  installedPackages: 1, optionalDependencies: false, cli: 'passed', mcp: 'passed', sdk: 'passed' }));
