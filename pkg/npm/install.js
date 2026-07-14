#!/usr/bin/env node
'use strict';

const crypto = require('crypto');
const fs = require('fs');
const https = require('https');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');

const packageVersion = require('./package.json').version;
const defaultReleaseBase = 'https://github.com/GoldeneyeZ/goldeneye-tool/releases/download';

function normalizeVersion(value) {
  const version = String(value).replace(/^v/, '');
  if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
    throw new Error(`invalid release version: ${value}`);
  }
  return version;
}

function platformSpec(platform = process.platform, arch = process.arch) {
  const platforms = { darwin: 'darwin', linux: 'linux', win32: 'windows' };
  const arches = { arm64: 'arm64', x64: 'x64' };
  const releasePlatform = platforms[platform];
  const releaseArch = arches[arch];
  if (!releasePlatform || !releaseArch) {
    throw new Error(`unsupported platform: ${platform}/${arch}`);
  }
  return {
    platform: releasePlatform,
    arch: releaseArch,
    extension: platform === 'win32' ? 'zip' : 'tar.gz',
    executable: platform === 'win32' ? 'goldeneye.exe' : 'goldeneye',
  };
}

function releaseAsset(versionValue, platform, arch, baseValue = defaultReleaseBase) {
  const version = normalizeVersion(versionValue);
  const base = String(baseValue).replace(/\/+$/, '');
  if (!base.startsWith('https://')) {
    throw new Error('release base URL must use HTTPS');
  }
  const spec = platformSpec(platform, arch);
  const name = `goldeneye-${spec.platform}-${spec.arch}.${spec.extension}`;
  return {
    ...spec,
    version,
    name,
    url: `${base}/v${version}/${name}`,
    checksumsUrl: `${base}/v${version}/checksums.txt`,
  };
}

function parseChecksums(text, assetName) {
  let found;
  for (const rawLine of String(text).split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line) continue;
    const match = /^([0-9a-fA-F]{64})\s+\*?(.+)$/.exec(line);
    if (!match) throw new Error(`malformed checksum line: ${line}`);
    if (path.basename(match[2].trim()) === assetName) {
      if (found) throw new Error(`duplicate checksum for ${assetName}`);
      found = match[1].toLowerCase();
    }
  }
  if (!found) throw new Error(`checksums.txt has no entry for ${assetName}`);
  return found;
}

function sha256(file) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(file));
  return hash.digest('hex');
}

function download(urlValue, destination, redirects = 0) {
  return new Promise((resolve, reject) => {
    const url = new URL(urlValue);
    if (url.protocol !== 'https:') {
      reject(new Error(`refusing non-HTTPS download: ${url}`));
      return;
    }
    if (redirects > 5) {
      reject(new Error(`too many redirects downloading ${url}`));
      return;
    }
    const request = https.get(url, {
      headers: { 'User-Agent': `goldeneye-tool-npm/${packageVersion}` },
    }, response => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        response.resume();
        const next = new URL(response.headers.location, url);
        if (next.protocol !== 'https:') {
          reject(new Error(`refusing redirect to non-HTTPS URL: ${next}`));
          return;
        }
        download(next, destination, redirects + 1).then(resolve, reject);
        return;
      }
      if (response.statusCode !== 200) {
        response.resume();
        reject(new Error(`download failed with HTTP ${response.statusCode}: ${url}`));
        return;
      }
      const output = fs.createWriteStream(destination, { flags: 'wx' });
      response.pipe(output);
      output.on('finish', () => output.close(resolve));
      output.on('error', reject);
    });
    request.on('error', reject);
  });
}

function validateEntries(entries) {
  for (const rawEntry of entries) {
    const entry = rawEntry.trim().replace(/\\/g, '/');
    if (!entry) continue;
    const parts = entry.split('/');
    if (entry.startsWith('/') || /^[A-Za-z]:/.test(entry) || parts.includes('..')) {
      throw new Error(`unsafe archive entry: ${rawEntry}`);
    }
  }
}

function run(command, args) {
  const result = spawnSync(command, args, { encoding: 'utf8' });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} failed: ${(result.stderr || result.stdout).trim()}`);
  }
  return result.stdout;
}

function quotePowerShell(value) {
  return `'${String(value).replace(/'/g, "''")}'`;
}

function extractVerifiedArchive(archive, destination, spec) {
  if (spec.extension === 'tar.gz') {
    validateEntries(run('tar', ['-tzf', archive]).split(/\r?\n/));
    run('tar', ['-xzf', archive, '-C', destination]);
    return;
  }
  const listScript = [
    "$ErrorActionPreference = 'Stop'",
    'Add-Type -AssemblyName System.IO.Compression.FileSystem',
    `$zip = [IO.Compression.ZipFile]::OpenRead(${quotePowerShell(archive)})`,
    'try { $zip.Entries | ForEach-Object { $_.FullName } } finally { $zip.Dispose() }',
  ].join('; ');
  validateEntries(run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', listScript]).split(/\r?\n/));
  const extractScript = [
    "$ErrorActionPreference = 'Stop'",
    `Expand-Archive -LiteralPath ${quotePowerShell(archive)} -DestinationPath ${quotePowerShell(destination)} -Force`,
  ].join('; ');
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', extractScript]);
}

async function install() {
  if (process.env.GOLDENEYE_SKIP_INSTALL === '1') {
    console.log('goldeneye: binary download skipped by GOLDENEYE_SKIP_INSTALL');
    return;
  }
  const version = process.env.GOLDENEYE_VERSION || packageVersion;
  const asset = releaseAsset(
    version,
    process.platform,
    process.arch,
    process.env.GOLDENEYE_RELEASE_BASE_URL || defaultReleaseBase,
  );
  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'goldeneye-install-'));
  try {
    const archive = path.join(temporary, asset.name);
    const checksums = path.join(temporary, 'checksums.txt');
    await download(asset.checksumsUrl, checksums);
    await download(asset.url, archive);
    const expected = parseChecksums(fs.readFileSync(checksums, 'utf8'), asset.name);
    const actual = sha256(archive);
    if (actual !== expected) {
      throw new Error(`checksum mismatch for ${asset.name}: expected ${expected}, got ${actual}`);
    }
    const unpacked = path.join(temporary, 'unpacked');
    fs.mkdirSync(unpacked);
    extractVerifiedArchive(archive, unpacked, asset);
    const source = path.join(unpacked, asset.executable);
    if (!fs.statSync(source).isFile()) {
      throw new Error(`archive does not contain ${asset.executable}`);
    }
    const destinationDirectory = path.join(__dirname, 'bin');
    fs.mkdirSync(destinationDirectory, { recursive: true });
    const destination = path.join(destinationDirectory, asset.executable);
    fs.copyFileSync(source, destination);
    if (process.platform !== 'win32') fs.chmodSync(destination, 0o755);
    console.log(`goldeneye: installed verified ${asset.name}`);
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

module.exports = {
  defaultReleaseBase,
  normalizeVersion,
  parseChecksums,
  platformSpec,
  releaseAsset,
  validateEntries,
};

if (require.main === module) {
  install().catch(error => {
    console.error(`goldeneye: installation failed: ${error.message}`);
    process.exitCode = 1;
  });
}
