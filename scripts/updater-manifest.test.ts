import assert from 'node:assert/strict';
import test from 'node:test';
import {
  matchStableUpdaterAsset,
  requiredStableUpdaterTargets,
} from './updater-manifest.ts';

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

test('maps current stable Windows updater assets', () => {
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-setup.nsis.zip'),
    {
      kind: 'url',
      targets: ['win64', 'windows-x86_64'],
    },
  );
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-setup.nsis.zip.sig'),
    {
      kind: 'signature',
      targets: ['win64', 'windows-x86_64'],
    },
  );
});

test('maps Windows arm64 updater assets', () => {
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_arm64-setup.nsis.zip'),
    {
      kind: 'url',
      targets: ['windows-aarch64'],
    },
  );
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_arm64-setup.nsis.zip.sig'),
    {
      kind: 'signature',
      targets: ['windows-aarch64'],
    },
  );
});

test('ignores Windows installer executables as updater assets', () => {
  assert.equal(matchStableUpdaterAsset('Chimera_0.23.1_x64-setup.exe'), null);
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.23.1_arm64-setup.exe.sig'),
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

test('maps Linux AppImage updater archives', () => {
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_amd64.AppImage.tar.gz'),
    {
      kind: 'url',
      targets: ['linux', 'linux-x86_64'],
    },
  );
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_amd64.AppImage.tar.gz.sig'),
    {
      kind: 'signature',
      targets: ['linux', 'linux-x86_64'],
    },
  );
});

test('ignores release assets that are not updater payloads', () => {
  for (const name of [
    'Chimera_0.23.1_aarch64.dmg',
    'Chimera_0.23.1_x64-setup.exe',
    'Chimera_0.23.1_amd64.AppImage',
    'Chimera_0.23.1_amd64.deb',
    'Chimera-0.23.1-1.x86_64.rpm',
  ]) {
    assert.equal(matchStableUpdaterAsset(name), null, name);
  }
});

test('respects fixed-webview asset filtering', () => {
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-fixed-webview-setup.nsis.zip'),
    null,
  );
  assert.deepEqual(
    matchStableUpdaterAsset(
      'Chimera_0.23.1_x64-fixed-webview-setup.nsis.zip',
      true,
    ),
    {
      kind: 'url',
      targets: ['win64', 'windows-x86_64'],
    },
  );
});
