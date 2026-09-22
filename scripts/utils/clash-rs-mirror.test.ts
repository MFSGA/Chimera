import assert from 'node:assert/strict';
import test from 'node:test';
import { getClashRustAlphaInfo, getClashRustInfo } from './resource.ts';

const MIRROR =
  'https://github.com/MFSGA/Chimera_Service/releases/download/deps-clash-rs-0.10.8';

test('clash-rs release targets resolve to the pinned dependency mirror', () => {
  const cases = [
    {
      platform: 'win32',
      arch: 'x64',
      host: 'x86_64-pc-windows-msvc',
      stable: 'clash-rs-x86_64-pc-windows-msvc.exe',
      alpha: 'clash-rs-alpha-x86_64-pc-windows-msvc.exe',
    },
    {
      platform: 'linux',
      arch: 'x64',
      host: 'x86_64-unknown-linux-gnu',
      stable: 'clash-rs-x86_64-unknown-linux-gnu-static-crt',
      alpha: 'clash-rs-alpha-x86_64-unknown-linux-gnu-static-crt',
    },
    {
      platform: 'darwin',
      arch: 'arm64',
      host: 'aarch64-apple-darwin',
      stable: 'clash-rs-aarch64-apple-darwin',
      alpha: 'clash-rs-alpha-aarch64-apple-darwin',
    },
  ] as const;

  for (const current of cases) {
    const stable = getClashRustInfo({
      platform: current.platform,
      arch: current.arch,
      sidecarHost: current.host,
    });
    assert.equal(stable.version, 'v0.10.8');
    assert.equal(stable.downloadURL, `${MIRROR}/${current.stable}`);
    assert.equal(stable.checksumURL, `${stable.downloadURL}.sha256`);
    assert.equal(
      stable.targetFile,
      `clash-rs-${current.host}${current.platform === 'win32' ? '.exe' : ''}`,
    );

    const alpha = getClashRustAlphaInfo({
      platform: current.platform,
      arch: current.arch,
      sidecarHost: current.host,
    });
    assert.equal(alpha.version, '0.10.8-alpha+sha.b0538e8');
    assert.equal(alpha.downloadURL, `${MIRROR}/${current.alpha}`);
    assert.equal(alpha.checksumURL, `${alpha.downloadURL}.sha256`);
    assert.equal(
      alpha.targetFile,
      `clash-rs-alpha-${current.host}${current.platform === 'win32' ? '.exe' : ''}`,
    );
  }
});

test('clash-rs resolver no longer depends on the retired Watfaq release path', () => {
  const stable = getClashRustInfo({
    platform: 'win32',
    arch: 'x64',
    sidecarHost: 'x86_64-pc-windows-msvc',
  });
  const alpha = getClashRustAlphaInfo({
    platform: 'win32',
    arch: 'x64',
    sidecarHost: 'x86_64-pc-windows-msvc',
  });

  assert.equal(stable.downloadURL.includes('Watfaq/clash-rs'), false);
  assert.equal(alpha.downloadURL.includes('Watfaq/clash-rs'), false);
});
