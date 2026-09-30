import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { checkProductVersion, productVersion, syncProductVersion } from './product-version.mjs';

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'chat2db-version-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const documents = {
    'Cargo.toml': '[workspace.package]\nversion = "0.0.1"\n[workspace.dependencies]\nversion = "9.9.9"\n',
    'Cargo.lock': 'version = 4\n\n[[package]]\nname = "chat2db-core"\nversion = "0.1.0"\n\n[[package]]\nname = "dependency"\nversion = "0.1.0"\nsource = "registry+https://example.test"\n',
    'apps/chat2db-desktop/tauri.conf.json': { version: '0.1.0', app: { windows: [] } },
    'apps/frontend/package.json': { name: '@chat2db/frontend', version: '0.1.0' },
    'apps/frontend/package-lock.json': { version: '0.1.0', packages: { '': { version: '0.1.0' }, dependency: { version: '0.1.0' } } },
    'contracts/openapi/chat2db-v1.json': { info: { version: '0.0.1' } },
  };
  for (const [relativePath, contents] of Object.entries(documents)) {
    const path = join(root, relativePath);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, typeof contents === 'string' ? contents : JSON.stringify(contents));
  }
  return root;
}

test('sync uses the workspace version and preserves dependency versions and other configuration', (t) => {
  const root = fixture(t);
  assert.equal(productVersion(root), '0.0.1');
  assert.equal(syncProductVersion(root), '0.0.1');
  assert.equal(checkProductVersion(root, 'v0.0.1'), '0.0.1');
  const lock = readFileSync(join(root, 'Cargo.lock'), 'utf8');
  assert.match(lock, /name = "chat2db-core"\nversion = "0.0.1"/);
  assert.match(lock, /name = "dependency"\nversion = "0.1.0"/);
  const npm = JSON.parse(readFileSync(join(root, 'apps/frontend/package-lock.json'), 'utf8'));
  assert.equal(npm.packages.dependency.version, '0.1.0');
  const tauri = JSON.parse(readFileSync(join(root, 'apps/chat2db-desktop/tauri.conf.json'), 'utf8'));
  assert.deepEqual(tauri.app, { windows: [] });
});

test('check rejects a tag that would label the binaries with a different version', (t) => {
  const root = fixture(t);
  syncProductVersion(root);
  assert.throws(() => checkProductVersion(root, 'v0.1.0'), /does not match/);
});

for (const relativePath of [
  'apps/chat2db-desktop/tauri.conf.json',
  'apps/frontend/package.json',
  'apps/frontend/package-lock.json',
  'contracts/openapi/chat2db-v1.json',
]) {
  test(`check rejects version drift in ${relativePath}`, (t) => {
    const root = fixture(t);
    syncProductVersion(root);
    const path = join(root, relativePath);
    writeFileSync(path, readFileSync(path, 'utf8').replace('0.0.1', '0.2.0'));
    assert.throws(() => checkProductVersion(root), /expected 0.0.1/);
  });
}

test('check rejects stale Rust workspace package versions', (t) => {
  const root = fixture(t);
  syncProductVersion(root);
  const path = join(root, 'Cargo.lock');
  writeFileSync(path, readFileSync(path, 'utf8').replace('version = "0.0.1"', 'version = "0.1.0"'));
  assert.throws(() => checkProductVersion(root), /Cargo.lock chat2db-core/);
});
