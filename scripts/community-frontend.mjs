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
const SOURCE_DIR = join(ROOT_DIR, 'apps', 'community-client');
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

function verifySource() {
  if (!existsSync(join(SOURCE_DIR, 'package.json'))) {
    fail('vendored community client is missing at apps/community-client');
  }
  return { commit: lock.commit, tree: lock.tree };
}

function exportSource(destination) {
  replaceDirectory(destination);
  cpSync(SOURCE_DIR, destination, {
    recursive: true,
    filter: (src) => !/[\\/](node_modules|dist|\.umi|\.umi-production)([\\/]|$)/.test(src),
  });
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

// The pinned client renames and retires its own regression scripts between
// syncs. Run the ones this repository tracks that the pinned revision still
// defines instead of failing on a name upstream removed.
const COMMUNITY_TEST_SCRIPTS = [
  'test:chat-answer-update',
  'test:tree-title-highlight',
  'test:deep-clone',
  'test:ai-model-select',
  'test:export-connections',
  'test:host-file-transfer',
  'test:canvas-lifecycle',
  'test:result-error-boundary',
  'test:result-resource-activity',
  'test:result-table-lifecycle',
  'test:workspace-resource-activity',
];

function test() {
  const worktree = createWorktree('test');
  setupUmi(worktree);
  const scripts = JSON.parse(readFileSync(join(worktree, 'package.json'), 'utf8')).scripts ?? {};
  const selected = COMMUNITY_TEST_SCRIPTS.filter((name) => scripts[name]);
  if (!selected.length) {
    fail('the pinned client no longer exposes any tracked regression script');
  }
  for (const script of selected) {
    runYarn([script], worktree);
  }
}

/**
 * The pinned client's `prebuild`/`postbuild` hooks assume the upstream
 * monorepo layout (they scan `../chat2db-community-server`). Drop them in the
 * exported worktree so the product build only runs the Umi bundler.
 */
function stripUpstreamBuildHooks(worktree) {
  const packagePath = join(worktree, 'package.json');
  const pkg = JSON.parse(readFileSync(packagePath, 'utf8'));
  if (!pkg.scripts) {
    return;
  }
  delete pkg.scripts['prebuild:web:community'];
  delete pkg.scripts['postbuild:web:community'];
  writeFileSync(packagePath, `${JSON.stringify(pkg, null, 2)}\n`);
}

function build() {
  const worktree = createWorktree('build');
  stripUpstreamBuildHooks(worktree);
  setupUmi(worktree);
  runYarn(
    [
      'build:web:community',
      '--app_port=4200',
      '--public_path=./',
      `--app_version=${productVersion()}`,
    ],
    worktree,
    {
      APP_DISPLAY_NAME: 'Chat2DB Rust',
      APP_VERSION: productVersion(),
    },
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

/**
 * Serves the desktop bridge from the dev server so `umi dev` talks to the
 * Tauri backend instead of the HTTP proxy. The production build injects the
 * same script into the exported `index.html`.
 */
function injectTauriBridge(worktree) {
  mkdirSync(join(worktree, 'public'), { recursive: true });
  cpSync(TAURI_BRIDGE_PATH, join(worktree, 'public', TAURI_BRIDGE_NAME));
  const configPath = join(worktree, '.umirc.ts');
  const config = readFileSync(configPath, 'utf8');
  const bridge = `{ src: '/${TAURI_BRIDGE_NAME}' }`;
  const markers = [
    ['headScripts: [createStorageVersionScript()],', `headScripts: [createStorageVersionScript(), ${bridge}],`],
    ['headScripts: createHeadScripts(),', `headScripts: [...createHeadScripts(), ${bridge}],`],
  ];
  const match = markers.find(([from]) => config.includes(from));
  if (!match) {
    fail('community dev config no longer exposes a known headScripts entry point');
  }
  writeFileSync(configPath, config.replace(match[0], match[1]));
}

function dev() {
  const worktree = createWorktree('dev');
  injectTauriBridge(worktree);
  runYarn(
    ['umi', 'dev', '--public_path=/', '--proxy_target=http://127.0.0.1:4200'],
    worktree,
    {
      UMI_ENV: 'community',
      APP_NAME: 'chat2db-community',
      APP_DISPLAY_NAME: 'Chat2DB Rust',
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
      `community frontend: using vendored source apps/community-client (synced from ${lock.repository}@${lock.commit})`,
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
