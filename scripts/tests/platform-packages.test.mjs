import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { assemble, binaries, platforms } from '../package-npm.mjs';
const { resolveBinary } = createRequire(import.meta.url)('../../npm/launcher.cjs');
const repo = path.resolve(import.meta.dirname, '../..');

for (const platform of platforms) {
  test(`package and select ${platform}; reject mismatched architecture and tampering`, t => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'magicvault-platform-'));
    t.after(() => fs.rmSync(root, {recursive: true, force: true}));
    const bin = path.join(root, 'bin');
    fs.mkdirSync(bin);
    const [system, arch] = platform.split('-');
    const suffix = system === 'win32' ? '.exe' : '';
    const bytes = Buffer.alloc(256);
    if (system === 'darwin') {
      bytes.writeUInt32LE(0xfeedfacf); bytes.writeUInt32LE(arch === 'arm64' ? 0x0100000c : 0x01000007, 4);
    } else if (system === 'linux') {
      Buffer.from([0x7f, 69, 76, 70, 2, 1]).copy(bytes);
      bytes.writeUInt16LE(arch === 'arm64' ? 183 : 62, 18);
    } else {
      bytes.write('MZ'); bytes.writeUInt32LE(128, 60); bytes.writeUInt32LE(0x00004550, 128);
      bytes.writeUInt16LE(arch === 'arm64' ? 0xaa64 : 0x8664, 132); bytes.writeUInt16LE(0x20b, 152);
    }
    for (const binary of binaries) fs.writeFileSync(path.join(bin, binary + suffix), bytes);
    const wrongOutput = path.join(root, 'wrong');
    const opposite = `${system}-${arch === 'arm64' ? 'x64' : 'arm64'}`;
    assert.throws(() => assemble({repo, binaryDir:bin, output:wrongOutput, scope:'@magicvault-local', platform:opposite}));
    assert.equal(fs.existsSync(wrongOutput), false);
    const output = path.join(root, 'packages');
    const result = assemble({repo, binaryDir:bin, output, scope:'@magicvault-local', platform});
    const modules = path.join(output, 'launcher/node_modules/@magicvault-local');
    fs.mkdirSync(modules, {recursive:true});
    const native = path.join(modules, `magicvault-${platform}`);
    fs.renameSync(path.join(output, 'native'), native);
    const packageFile = path.join(output, 'launcher/package.json');
    const metadata = JSON.parse(fs.readFileSync(packageFile));
    assert.equal(Object.keys(metadata.magicvault.platforms).length, platforms.length);
    assert.equal(metadata.optionalDependencies[result.nativeName], result.version);
    for (const command of ['magicvault', 'magicvault-mcp']) {
      assert.equal(resolveBinary(command, packageFile, system, arch), fs.realpathSync(path.join(native, `bin/${command}${suffix}`)));
    }
    assert(fs.existsSync(path.join(native, `bin/magicvault-prompt${suffix}`)));
    fs.appendFileSync(path.join(native, `bin/magicvault${suffix}`), 'tamper');
    assert.throws(() => resolveBinary('magicvault', packageFile, system, arch));
  });
}
