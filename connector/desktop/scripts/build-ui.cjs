#!/usr/bin/env node
// Cross-platform build:ui script — works on Windows, macOS, Linux
const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..');
const dist = path.join(root, 'dist');
const distSrc = path.join(dist, 'src');

// Clean and recreate dist/
if (fs.existsSync(dist)) fs.rmSync(dist, { recursive: true, force: true });
fs.mkdirSync(distSrc, { recursive: true });

// Copy files
fs.copyFileSync(path.join(root, 'index.html'), path.join(dist, 'index.html'));
fs.copyFileSync(path.join(root, 'src', 'main.js'), path.join(distSrc, 'main.js'));
fs.copyFileSync(path.join(root, 'src', 'styles.css'), path.join(distSrc, 'styles.css'));

console.log('UI build complete: dist/index.html + dist/src/main.js + dist/src/styles.css');
