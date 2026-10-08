#!/usr/bin/env node

import { spawnSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';
import { productVersion } from './product-version.mjs';

const SCRIPT_DIR = dirname(fileURLToPath(import.meta.url));
const ROOT_DIR = resolve(SCRIPT_DIR, '..');
const LOCK_PATH = join(SCRIPT_DIR, 'community-frontend.lock.json');
const lock = JSON.parse(readFileSync(LOCK_PATH, 'utf8'));
const SUBMODULE_DIR = join(ROOT_DIR, lock.submodulePath);
const TARGET_ROOT = join(
  ROOT_DIR,
  'target',
  process.platform === 'win32' ? 'cf' : 'community-frontend',
);
const VERSION_ROOT = join(
  TARGET_ROOT,
  process.platform === 'win32' ? lock.tree.slice(0, 12) : lock.tree,
);
const DEPENDENCIES_DIR = join(VERSION_ROOT, 'dependencies');
const YARN_JS = join(ROOT_DIR, 'apps', 'frontend', 'node_modules', 'yarn', 'bin', 'yarn.js');
const OUTPUT_DIR = join(ROOT_DIR, 'apps', 'frontend', 'dist');
const TAURI_BRIDGE_PATH = join(SCRIPT_DIR, 'community-tauri-bridge.js');
const TAURI_BRIDGE_NAME = 'chat2db-rust-tauri-bridge.js';

function fail(message) {
  console.error(`community frontend: ${message}`);
  process.exit(1);
}

function capture(command, args, cwd = ROOT_DIR) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (result.error) {
    fail(`${command} could not start: ${result.error.message}`);
  }
  if (result.status !== 0) {
    const detail = (result.stderr || '').trim() || (result.stdout || '').trim();
    fail(`${command} ${args.join(' ')} failed${detail ? `: ${detail}` : ''}`);
  }
  return result.stdout.trim();
}

function run(command, args, { cwd = ROOT_DIR, env = {} } = {}) {
  const result = spawnSync(command, args, {
    cwd,
    env: { ...process.env, ...env },
    stdio: 'inherit',
  });
  if (result.error) {
    fail(`${command} could not start: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`${command} ${args.join(' ')} exited with status ${result.status}`);
  }
}

function assertSafeTarget(target) {
  const resolvedTarget = resolve(target);
  const relativeTarget = relative(TARGET_ROOT, resolvedTarget);
  if (
    resolvedTarget === TARGET_ROOT ||
    relativeTarget === '' ||
    relativeTarget === '..' ||
    relativeTarget.startsWith(`..${sep}`) ||
    isAbsolute(relativeTarget)
  ) {
    fail(`refusing to replace unsafe target path ${resolvedTarget}`);
  }
}

function replaceDirectory(target) {
  assertSafeTarget(target);
  rmSync(target, { recursive: true, force: true });
  mkdirSync(target, { recursive: true });
}

function hasLockedCommit() {
  const result = spawnSync('git', ['-C', SUBMODULE_DIR, 'cat-file', '-e', `${lock.commit}^{commit}`]);
  return result.status === 0;
}

// The submodule checkout tracks the Java compatibility baseline recorded in
// `third_party/community-h2-classpath.lock`, so the locked frontend commit is
// fetched into the submodule object store instead of being checked out.
function ensureLockedCommit() {
  if (hasLockedCommit()) {
    return;
  }
  spawnSync('git', ['-C', SUBMODULE_DIR, 'fetch', '--no-tags', 'origin', lock.commit], {
    stdio: 'inherit',
  });
  if (!hasLockedCommit()) {
    run('git', [
      '-C',
      SUBMODULE_DIR,
      'fetch',
      '--no-tags',
      'origin',
      '+refs/heads/*:refs/remotes/origin/*',
      '+refs/tags/*:refs/tags/*',
    ]);
  }
  if (!hasLockedCommit()) {
    fail(`locked Community commit ${lock.commit} is unavailable in ${lock.submodulePath}`);
  }
}

function verifySource() {
  if (!existsSync(join(SUBMODULE_DIR, '.git'))) {
    fail(`submodule is unavailable; run git submodule update --init --recursive ${lock.submodulePath}`);
  }
  ensureLockedCommit();

  const tree = capture('git', ['rev-parse', `${lock.commit}:${lock.sourcePath}`], SUBMODULE_DIR);
  if (tree !== lock.tree) {
    fail(`source tree is ${tree}; expected byte-level tree ${lock.tree}`);
  }

  const status = capture(
    'git',
    ['status', '--porcelain=v1', '--untracked-files=all'],
    SUBMODULE_DIR,
  );
  if (status !== '') {
    fail(`submodule worktree is not clean:\n${status}`);
  }

  return { commit: lock.commit, tree };
}

function exportSource(destination, sourcePath = lock.sourcePath) {
  ensureLockedCommit();
  replaceDirectory(destination);
  mkdirSync(VERSION_ROOT, { recursive: true });
  const archiveName = `source-${process.pid}.tar`;
  const archivePath = join(VERSION_ROOT, archiveName);
  const archiveOutput = relative(SUBMODULE_DIR, archivePath).split(sep).join('/');
  rmSync(archivePath, { force: true });
  run(
    'git',
    [
      '-C',
      SUBMODULE_DIR,
      'archive',
      '--format=tar',
      `--output=${archiveOutput}`,
      lock.commit,
      sourcePath,
    ],
    { cwd: VERSION_ROOT },
  );
  const destinationPath = relative(VERSION_ROOT, destination).split(sep).join('/');
  run('tar', ['-xf', archiveName, '--strip-components=1', '-C', destinationPath], {
    cwd: VERSION_ROOT,
  });
  rmSync(archivePath, { force: true });
}

// Upstream's community boundary check scans the sibling server tree, so every
// exported worktree needs the monorepo layout next to it.
function exportCommunityServer() {
  exportSource(join(VERSION_ROOT, 'work', 'chat2db-community-server'), 'chat2db-community-server');
}

function runYarn(args, cwd, env = {}) {
  if (!existsSync(YARN_JS)) {
    fail('pinned Yarn is missing; run npm ci in apps/frontend first');
  }
  run(process.execPath, [YARN_JS, ...args], { cwd, env });
}

function ensureDependencies() {
  const stampPath = join(DEPENDENCIES_DIR, '.chat2db-install.json');
  const expectedStamp = JSON.stringify(
    { tree: lock.tree, packageManager: lock.packageManager },
    null,
    2,
  );
  if (
    existsSync(join(DEPENDENCIES_DIR, 'node_modules')) &&
    existsSync(stampPath) &&
    readFileSync(stampPath, 'utf8') === `${expectedStamp}\n`
  ) {
    return;
  }

  exportSource(DEPENDENCIES_DIR);
  runYarn(['install', '--frozen-lockfile', '--non-interactive'], DEPENDENCIES_DIR);
  writeFileSync(stampPath, `${expectedStamp}\n`);
}

function createWorktree(name) {
  ensureDependencies();
  exportCommunityServer();
  const worktree = join(VERSION_ROOT, 'work', name);
  exportSource(worktree);
  symlinkSync(
    join(DEPENDENCIES_DIR, 'node_modules'),
    join(worktree, 'node_modules'),
    process.platform === 'win32' ? 'junction' : 'dir',
  );
  return worktree;
}

function setupUmi(worktree) {
  runYarn(['umi', 'setup'], worktree, {
    UMI_ENV: 'community',
    APP_NAME: 'chat2db-community',
  });
}

function test() {
  const worktree = createWorktree('test');
  setupUmi(worktree);
  // Upstream deleted the other per-behavior scripts this step used to pin.
  // `prebuild:web:community` now runs the full upstream test battery before
  // every community build, so only the surviving checks stay pinned here.
  runYarn(['test:tree-title-highlight'], worktree);
  runYarn(['test:ai-model-select'], worktree);
  runYarn(['test:export-connections'], worktree);
}

function build() {
  const worktree = createWorktree('build');
  setupUmi(worktree);
  runYarn(
    [
      'build:web:community',
      '--app_port=4200',
      '--public_path=./',
      `--app_version=${productVersion()}`,
    ],
    worktree,
  );
  const builtDist = join(worktree, 'dist');
  if (!existsSync(join(builtDist, 'index.html'))) {
    fail('upstream build completed without dist/index.html');
  }
  rmSync(OUTPUT_DIR, { recursive: true, force: true });
  cpSync(builtDist, OUTPUT_DIR, { recursive: true });
  cpSync(TAURI_BRIDGE_PATH, join(OUTPUT_DIR, TAURI_BRIDGE_NAME));
  const indexPath = join(OUTPUT_DIR, 'index.html');
  const indexHtml = readFileSync(indexPath, 'utf8');
  const bridgeTag = `<script src="./${TAURI_BRIDGE_NAME}"></script>`;
  if (!indexHtml.includes('</head>')) {
    fail('upstream index.html does not contain a closing head tag');
  }
  writeFileSync(indexPath, indexHtml.replace('</head>', `${bridgeTag}</head>`));
  writeFileSync(
    join(OUTPUT_DIR, 'chat2db-community-provenance.json'),
    `${JSON.stringify(lock, null, 2)}\n`,
  );
  console.log(`community frontend: built ${lock.commit} into ${relative(ROOT_DIR, OUTPUT_DIR)}`);
}

function dev() {
  const worktree = createWorktree('dev');
  runYarn(
    ['umi', 'dev', '--public_path=/', '--proxy_target=http://127.0.0.1:4200'],
    worktree,
    {
      UMI_ENV: 'community',
      APP_NAME: 'chat2db-community',
      APP_VERSION: productVersion(),
      DISABLE_MFSU: 'true',
      HOST: '127.0.0.1',
      UMI_DEV_SERVER_COMPRESS: 'none',
      PORT: process.env.CHAT2DB_FRONTEND_PORT || '4210',
    },
  );
}

const command = process.argv[2];
verifySource();

switch (command) {
  case 'verify':
    console.log(
      `community frontend: verified ${lock.repository}@${lock.commit}:${lock.sourcePath} (${lock.tree})`,
    );
    break;
  case 'test':
    test();
    break;
  case 'build':
    build();
    break;
  case 'dev':
    dev();
    break;
  default:
    fail('usage: community-frontend.mjs <verify|test|build|dev>');
}
