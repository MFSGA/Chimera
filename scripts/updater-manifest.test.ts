import assert from 'node:assert/strict';
import test from 'node:test';
import { matchStableUpdaterAsset } from './updater-manifest.ts';

test('maps current stable Windows updater assets', () => {
  assert.deepEqual(matchStableUpdaterAsset('Chimera_0.23.1_x64-setup.exe'), {
    kind: 'url',
    targets: ['win64', 'windows-x86_64'],
  });
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-setup.exe.sig'),
    {
      kind: 'signature',
      targets: ['win64', 'windows-x86_64'],
    },
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

test('ignores release assets that are not updater payloads', () => {
  for (const name of [
    'Chimera_0.23.1_aarch64.dmg',
    'Chimera_0.23.1_amd64.AppImage',
    'Chimera_0.23.1_amd64.deb',
    'Chimera-0.23.1-1.x86_64.rpm',
  ]) {
    assert.equal(matchStableUpdaterAsset(name), null, name);
  }
});

test('respects fixed-webview asset filtering', () => {
  assert.equal(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-fixed-webview-setup.exe'),
    null,
  );
  assert.deepEqual(
    matchStableUpdaterAsset('Chimera_0.23.1_x64-fixed-webview-setup.exe', true),
    {
      kind: 'url',
      targets: ['win64', 'windows-x86_64'],
    },
  );
});
