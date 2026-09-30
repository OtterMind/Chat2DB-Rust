#!/usr/bin/env node

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT_DIR = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const JSON_VERSIONS = [
  ['apps/chat2db-desktop/tauri.conf.json', ['version']],
  ['apps/frontend/package.json', ['version']],
  ['apps/frontend/package-lock.json', ['version']],
  ['apps/frontend/package-lock.json', ['packages', '', 'version']],
];

export function productVersion(root = ROOT_DIR) {
  const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
  const section = cargo.split(/^\[/m).find((entry) => entry.startsWith('workspace.package]'));
  const version = section?.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (!version) throw new Error('Cargo.toml must declare workspace.package.version');
  return version;
}

function workspaceLockVersions(root, visit) {
  const path = join(root, 'Cargo.lock');
  const contents = readFileSync(path, 'utf8');
  return contents.replace(/^\[\[package\]\][\s\S]*?(?=^\[\[package\]\]|$(?![\s\S]))/gm, (block) => {
    const name = block.match(/^name = "(chat2db[^"]*)"/m)?.[1];
    if (!name || /^source = /m.test(block)) return block;
    return visit(block, name);
  });
}

export function syncProductVersion(root = ROOT_DIR) {
  const version = productVersion(root);
  for (const [relativePath, keys] of JSON_VERSIONS) {
    const path = join(root, relativePath);
    const document = JSON.parse(readFileSync(path, 'utf8'));
    const parent = keys.slice(0, -1).reduce((value, key) => value[key], document);
    parent[keys.at(-1)] = version;
    writeFileSync(path, `${JSON.stringify(document, null, 2)}\n`);
  }
  const lock = workspaceLockVersions(root, (block) =>
    block.replace(/^version = "[^"]+"/m, `version = "${version}"`));
  writeFileSync(join(root, 'Cargo.lock'), lock);
  return version;
}

export function checkProductVersion(root = ROOT_DIR, tag) {
  const version = productVersion(root);
  const projections = [
    ...JSON_VERSIONS,
    ['contracts/openapi/chat2db-v1.json', ['info', 'version']],
  ];
  for (const [relativePath, keys] of projections) {
    const document = JSON.parse(readFileSync(join(root, relativePath), 'utf8'));
    const actual = keys.reduce((value, key) => value[key], document);
    if (actual !== version) {
      throw new Error(`${relativePath} ${keys.join('.')} is ${actual}; expected ${version}`);
    }
  }
  workspaceLockVersions(root, (block, name) => {
    const actual = block.match(/^version = "([^"]+)"/m)?.[1];
    if (actual !== version) throw new Error(`Cargo.lock ${name} is ${actual}; expected ${version}`);
    return block;
  });
  if (tag !== undefined && tag !== `v${version}`) {
    throw new Error(`release tag ${tag} does not match product version v${version}`);
  }
  return version;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    switch (process.argv[2]) {
      case undefined:
        console.log(productVersion());
        break;
      case '--sync':
        console.log(`synchronized product version ${syncProductVersion()}; run make generate-contracts`);
        break;
      case '--check':
        console.log(`verified product version ${checkProductVersion(
          ROOT_DIR,
          process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : undefined,
        )}`);
        break;
      default:
        throw new Error('usage: product-version.mjs [--sync|--check]');
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
