import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import {
  matchStableUpdaterAsset,
  releaseAssetDownloadUrl,
  requiredStableUpdaterTargets,
} from './updater-manifest.ts';

test('builds stable release URLs from the tag instead of draft asset metadata', () => {
  assert.equal(
    releaseAssetDownloadUrl(
      'MFSGA',
      'Chimera',
      'v0.27.1',
      'Chimera_0.27.1_amd64.AppImage',
    ),
    'https://github.com/MFSGA/Chimera/releases/download/v0.27.1/Chimera_0.27.1_amd64.AppImage',
  );
  assert.equal(
    releaseAssetDownloadUrl(
      'MFSGA',
      'Chimera',
      'v0.27.1+meta',
      'asset one.zip',
    ),
    'https://github.com/MFSGA/Chimera/releases/download/v0.27.1%2Bmeta/asset%20one.zip',
  );
});

test('requires all release targets for standard updates', () => {
  assert.deepEqual(requiredStableUpdaterTargets(), [
    'windows-x86_64',
    'windows-aarch64',
    'darwin',
    'darwin-aarch64',
    'linux',
  ]);
});

test('requires only Windows x64 for fixed WebView updates', () => {
  assert.deepEqual(requiredStableUpdaterTargets(true), ['windows-x86_64']);
});

test('maps current stable Windows x64 updater assets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera_0.27.1_x64-setup.exe'), {
    kind: 'url',
    targets: ['win64', 'windows-x86_64'],
  });
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.27.1_x64-setup.exe.sig'),
    {
      kind: 'signature',
      targets: ['win64', 'windows-x86_64'],
    },
  );
});

test('maps Windows arm64 updater assets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera_0.27.1_arm64-setup.exe'), {
    kind: 'url',
    targets: ['windows-aarch64'],
  });
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.27.1_arm64-setup.exe.sig'),
    {
      kind: 'signature',
      targets: ['windows-aarch64'],
    },
  );
});

test('ignores legacy NSIS archives for the current Tauri updater format', () => {
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.27.1_x64-setup.nsis.zip'),
    null,
  );
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.27.1_arm64-setup.nsis.zip.sig'),
    null,
  );
});

test('maps current stable macOS arm updater assets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera.aarch64.app.tar.gz'), {
    kind: 'url',
    targets: ['darwin-aarch64'],
  });
  assert.deepEqual(matchStableUpdaterAsset('Chimera.aarch64.app.tar.gz.sig'), {
    kind: 'signature',
    targets: ['darwin-aarch64'],
  });
});

test('maps intel macOS updater assets to Tauri compatibility targets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera.x64.app.tar.gz'), {
    kind: 'url',
    targets: ['darwin', 'darwin-intel', 'darwin-x86_64'],
  });
  assert.deepEqual(matchStableUpdaterAsset('Chimera.x64.app.tar.gz.sig'), {
    kind: 'signature',
    targets: ['darwin', 'darwin-intel', 'darwin-x86_64'],
  });
});

test('maps current stable Linux AppImage updater assets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera_0.27.1_amd64.AppImage'), {
    kind: 'url',
    targets: ['linux', 'linux-x86_64'],
  });
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.27.1_amd64.AppImage.sig'),
    {
      kind: 'signature',
      targets: ['linux', 'linux-x86_64'],
    },
  );
});

test('ignores legacy AppImage archives for the current Tauri updater format', () => {
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.27.1_amd64.AppImage.tar.gz'),
    null,
  );
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.27.1_amd64.AppImage.tar.gz.sig'),
    null,
  );
});

test('ignores release assets that are not updater payloads', () => {
  for (const name of [
    'Chimera_0.23.1_aarch64.dmg',
    'Chimera_0.23.1_x64.msi',
    'Chimera_0.23.1_amd64.deb',
    'Chimera-0.23.1-1.x86_64.rpm',
  ]) {
    assert.equal(matchStableUpdaterAsset(name), null, name);
  }
});

test('uploads the Tauri v2 Linux updater signature with the AppImage', () => {
  const workflow = readFileSync(
    new URL('../.github/workflows/deps-build-linux.yaml', import.meta.url),
    'utf8',
  );

  assert.match(workflow, /\.\/backend\/target\/\*\*\/\*\.AppImage\.sig/);
});

test('respects fixed-webview asset filtering', () => {
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.27.1_x64-fixed-webview-setup.exe'),
    null,
  );
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.27.1_x64-fixed-webview-setup.exe', true),
    {
      kind: 'url',
      targets: ['win64', 'windows-x86_64'],
    },
  );
});
