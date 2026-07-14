#!/usr/bin/env node
'use strict';

const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');

const executable = process.platform === 'win32' ? 'goldeneye.exe' : 'goldeneye';
const binary = path.join(__dirname, 'bin', executable);

if (!fs.existsSync(binary)) {
  const installer = spawnSync(process.execPath, [path.join(__dirname, 'install.js')], {
    stdio: 'inherit',
  });
  if (installer.error) {
    console.error(`goldeneye: installer failed: ${installer.error.message}`);
    process.exit(1);
  }
  if (installer.status !== 0 || !fs.existsSync(binary)) {
    console.error('goldeneye: installer did not produce a verified binary');
    process.exit(installer.status || 1);
  }
}

const child = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
if (child.error) {
  console.error(`goldeneye: failed to start Rust binary: ${child.error.message}`);
  process.exit(1);
}
process.exit(child.status === null ? 1 : child.status);
