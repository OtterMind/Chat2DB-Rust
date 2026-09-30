#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkProductVersion } from './product-version.mjs';

const ROOT_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const PLATFORM_PACKAGES = {
  'macos-arm64': ['.app.zip', '.dmg'],
  'macos-x64': ['.app.zip', '.dmg'],
  'windows-x86_64': ['.exe', '.msi'],
  'linux-x86_64': ['.AppImage', '.deb', '.rpm'],
  'linux-arm64': ['.AppImage', '.deb', '.rpm'],
};

async function sha256(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

export async function prepareRelease({ artifactsDir, outputDir, version, commit, communityCommit }) {
  const files = new Map();
  const manifests = [];
  for (const [platform, extensions] of Object.entries(PLATFORM_PACKAGES)) {
    const directory = join(artifactsDir, `Chat2DB-Rust-Desktop-${platform}-${commit}`);
    const manifest = await readFile(join(directory, 'BUILD-MANIFEST.txt'), 'utf8');
    const fields = Object.fromEntries(
      manifest.split(/\r?\n/).filter((line) => line.includes('='))
        .map((line) => [line.slice(0, line.indexOf('=')), line.slice(line.indexOf('=') + 1)]),
    );
    for (const [key, expected] of Object.entries({
      version, git_commit: commit, community_commit: communityCommit,
    })) {
      if (fields[key] !== expected) throw new Error(`${platform} manifest ${key} must be ${expected}`);
    }
    const sums = await readFile(join(directory, 'SHA256SUMS'), 'utf8');
    const packages = [];
    for (const line of sums.trim().split(/\r?\n/)) {
      const entry = line.match(/^([a-f0-9]{64}) [ *](.+)$/i);
      if (!entry) throw new Error(`${platform} has an invalid SHA256SUMS entry`);
      const [, digest, rawName] = entry;
      const name = rawName.replace(/^\.\//, '');
      if (basename(name) !== name || name.includes('\\')) {
        throw new Error(`${platform} checksum filename must be a basename`);
      }
      if (files.has(name)) throw new Error(`duplicate release asset ${name}`);
      const source = join(directory, name);
      if (await sha256(source) !== digest.toLowerCase()) {
        throw new Error(`${platform} checksum mismatch for ${name}`);
      }
      packages.push(name);
      files.set(name, { source, digest: digest.toLowerCase() });
    }
    if (packages.length !== extensions.length || extensions.some((extension) =>
      packages.filter((name) => name.endsWith(extension)).length !== 1)) {
      throw new Error(`${platform} must contain exactly one package for each of ${extensions.join(', ')}`);
    }
    manifests.push({ name: `BUILD-MANIFEST-${platform}.txt`, source: join(directory, 'BUILD-MANIFEST.txt') });
  }

  await mkdir(outputDir, { recursive: true });
  if ((await readdir(outputDir)).length !== 0) throw new Error('release output directory must be empty');
  for (const [name, { source }] of files) await copyFile(source, join(outputDir, name));
  for (const { name, source } of manifests) await copyFile(source, join(outputDir, name));
  const combined = [...files].sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)
    .map(([name, { digest }]) => `${digest}  ${name}\n`).join('');
  await writeFile(join(outputDir, 'SHA256SUMS'), combined);
  return [...files.keys(), ...manifests.map(({ name }) => name), 'SHA256SUMS'];
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const [artifactsDir, outputDir] = process.argv.slice(2);
    if (!artifactsDir || !outputDir || !process.env.GITHUB_SHA) {
      throw new Error('usage: GITHUB_SHA=<source commit> node prepare-release.mjs <artifacts directory> <empty output directory>');
    }
    const version = checkProductVersion(ROOT_DIR, process.env.GITHUB_REF_NAME);
    const community = JSON.parse(await readFile(join(ROOT_DIR, 'scripts/community-frontend.lock.json'), 'utf8'));
    const assets = await prepareRelease({
      artifactsDir, outputDir, version, commit: process.env.GITHUB_SHA, communityCommit: community.commit,
    });
    console.log(`prepared ${assets.length} release assets for v${version}`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
