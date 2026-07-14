'use strict';

const assert = require('node:assert/strict');
const test = require('node:test');
const {
  normalizeVersion,
  parseChecksums,
  platformSpec,
  releaseAsset,
  validateEntries,
} = require('../install.js');

test('resolves the six release targets', () => {
  assert.deepEqual(platformSpec('linux', 'x64'), {
    platform: 'linux', arch: 'x64', extension: 'tar.gz', executable: 'goldeneye',
  });
  assert.equal(platformSpec('darwin', 'arm64').arch, 'arm64');
  assert.equal(platformSpec('win32', 'x64').executable, 'goldeneye.exe');
  assert.equal(platformSpec('win32', 'arm64').extension, 'zip');
  assert.throws(() => platformSpec('linux', 'ia32'), /unsupported/);
});

test('builds a versioned HTTPS asset URL', () => {
  const asset = releaseAsset('v1.2.3', 'darwin', 'arm64');
  assert.equal(asset.name, 'goldeneye-darwin-arm64.tar.gz');
  assert.match(asset.url, /\/v1\.2\.3\/goldeneye-darwin-arm64\.tar\.gz$/);
  assert.match(asset.checksumsUrl, /\/v1\.2\.3\/checksums\.txt$/);
  assert.equal(normalizeVersion('v1.2.3-rc.1'), '1.2.3-rc.1');
  assert.throws(() => releaseAsset('latest', 'linux', 'x64'), /invalid release version/);
  assert.throws(() => releaseAsset('1.2.3', 'linux', 'x64', 'http://example.test'), /HTTPS/);
});

test('requires one exact SHA-256 checksum', () => {
  const hash = 'a'.repeat(64);
  assert.equal(parseChecksums(`${hash}  goldeneye-linux-x64.tar.gz\n`, 'goldeneye-linux-x64.tar.gz'), hash);
  assert.throws(() => parseChecksums(`${hash}  another.tar.gz`, 'goldeneye-linux-x64.tar.gz'), /no entry/);
  assert.throws(() => parseChecksums('bad checksum', 'goldeneye-linux-x64.tar.gz'), /malformed/);
});

test('rejects archive traversal paths', () => {
  validateEntries(['goldeneye', 'LICENSE', 'docs/NOTICE']);
  assert.throws(() => validateEntries(['../goldeneye']), /unsafe/);
  assert.throws(() => validateEntries(['C:\\temp\\goldeneye.exe']), /unsafe/);
});
