import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, mkdir, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { PLATFORM_PACKAGES, prepareRelease } from './prepare-release.mjs';

async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(), 'chat2db-release-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const options = {
    artifactsDir: join(root, 'artifacts'), outputDir: join(root, 'release'),
    version: '0.0.1', commit: 'a'.repeat(40), communityCommit: 'b'.repeat(40),
  };
  const directories = {};
  for (const [platform, extensions] of Object.entries(PLATFORM_PACKAGES)) {
    const directory = join(options.artifactsDir, `Chat2DB-Rust-Desktop-${platform}-${options.commit}`);
    directories[platform] = directory;
    await mkdir(directory, { recursive: true });
    const sums = [];
    for (const extension of extensions) {
      const name = `Chat2DB Rust_0.0.1_${platform}${extension}`;
      const contents = `${platform} ${extension}`;
      await writeFile(join(directory, name), contents);
      sums.push(`${createHash('sha256').update(contents).digest('hex')}  ./${name}`);
    }
    await writeFile(join(directory, 'SHA256SUMS'), `${sums.join('\n')}\n`);
    await writeFile(join(directory, 'BUILD-MANIFEST.txt'),
      `version=${options.version}\ngit_commit=${options.commit}\ncommunity_commit=${options.communityCommit}\n`);
  }
  return { options, directories };
}

test('all twelve packages and five distinct manifests survive aggregation and checksum verification', async (t) => {
  const { options } = await fixture(t);
  const assets = await prepareRelease(options);
  assert.equal(assets.length, 18);
  assert.equal((await readdir(options.outputDir)).length, 18);
  const sums = (await readFile(join(options.outputDir, 'SHA256SUMS'), 'utf8')).trim().split('\n');
  assert.equal(sums.length, 12);
  for (const line of sums) {
    const digest = line.slice(0, 64);
    const contents = await readFile(join(options.outputDir, line.slice(66)));
    assert.equal(createHash('sha256').update(contents).digest('hex'), digest);
  }
});

test('a missing platform prevents release preparation', async (t) => {
  const { options, directories } = await fixture(t);
  await rm(directories['linux-arm64'], { recursive: true });
  await assert.rejects(prepareRelease(options), /ENOENT/);
});

test('a modified installer prevents release preparation', async (t) => {
  const { options, directories } = await fixture(t);
  await writeFile(join(directories['macos-arm64'], 'Chat2DB Rust_0.0.1_macos-arm64.dmg'), 'modified');
  await assert.rejects(prepareRelease(options), /checksum mismatch/);
});

test('a missing package format prevents release preparation', async (t) => {
  const { options, directories } = await fixture(t);
  const path = join(directories['windows-x86_64'], 'SHA256SUMS');
  const sums = await readFile(path, 'utf8');
  await writeFile(path, `${sums.split('\n')[0]}\n`);
  await assert.rejects(prepareRelease(options), /must contain exactly one package/);
});

test('identical installer names across platforms cannot overwrite one another', async (t) => {
  const { options, directories } = await fixture(t);
  const directory = directories['macos-x64'];
  const oldName = 'Chat2DB Rust_0.0.1_macos-x64.dmg';
  const newName = 'Chat2DB Rust_0.0.1_macos-arm64.dmg';
  await rename(join(directory, oldName), join(directory, newName));
  const path = join(directory, 'SHA256SUMS');
  await writeFile(path, (await readFile(path, 'utf8')).replace(oldName, newName));
  await assert.rejects(prepareRelease(options), /duplicate release asset/);
});

for (const key of ['version', 'git_commit', 'community_commit']) {
  test(`a mismatched ${key} prevents mixing builds into one release`, async (t) => {
    const { options, directories } = await fixture(t);
    const path = join(directories['linux-x86_64'], 'BUILD-MANIFEST.txt');
    const manifest = await readFile(path, 'utf8');
    await writeFile(path, manifest.replace(new RegExp(`^${key}=.*$`, 'm'), `${key}=different`));
    await assert.rejects(prepareRelease(options), /manifest .* must be/);
  });
}
