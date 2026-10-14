import assert from 'node:assert/strict';
import { getChimeraClientAlphaInfo } from './binaries.ts';
import { resolveSidecar } from './resources.ts';
import type { ArchMapping, VersionManifest } from './types.ts';

const archMapping: ArchMapping = {
  'windows-i386': 'clash_chimera-i686-pc-windows-msvc.exe',
  'windows-x86_64': 'clash_chimera-x86_64-pc-windows-msvc.exe',
  'windows-arm64': 'clash_chimera-aarch64-pc-windows-msvc.exe',
  'linux-aarch64': 'clash_chimera-aarch64-unknown-linux-gnu',
  'linux-amd64': 'clash_chimera-x86_64-unknown-linux-gnu',
  'linux-i386': 'clash_chimera-i686-unknown-linux-musl',
  'linux-armv7': 'clash_chimera-armv7-unknown-linux-gnueabi',
  'linux-armv7hf': 'clash_chimera-armv7-unknown-linux-gnueabihf',
  'darwin-arm64': 'chimera-client-{}-aarch64-apple-darwin.tar.gz',
  'darwin-x64': 'chimera-client-{}-x86_64-apple-darwin.tar.gz',
};

const versionManifest = {
  manifest_version: 1,
  latest: {
    mihomo: '',
    mihomo_alpha: '',
    clash_rs: '',
    chimera_client: 'v0.26.3',
    chimera_client_alpha: '0.26.3-alpha+sha.406b0e0',
    clash_premium: '',
    clash_rs_alpha: '',
  },
  arch_template: {
    mihomo: archMapping,
    mihomo_alpha: archMapping,
    clash_rs: archMapping,
    chimera_client: archMapping,
    chimera_client_alpha: archMapping,
    clash_premium: archMapping,
    clash_rs_alpha: archMapping,
  },
  updated_at: '2026-10-14T00:00:00Z',
} satisfies VersionManifest;

Deno.test(
  'Chimera Client Alpha retries with current rolling release assets',
  async () => {
    let resolveCalls = 0;
    const binary = getChimeraClientAlphaInfo({
      versionManifest,
      platform: 'darwin',
      arch: 'darwin-arm64',
      sidecarHost: 'aarch64-apple-darwin',
      resolveLatest: async () => {
        resolveCalls++;
        return {
          name: 'chimera_client_alpha',
          version: '0.26.4-alpha+sha.abcdef0',
          archMapping,
        };
      },
    });

    assert.equal(binary.version, '0.26.3-alpha+sha.406b0e0');
    assert.equal(
      binary.downloadURL,
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.3-aarch64-apple-darwin.tar.gz',
    );

    const attempts: string[] = [];
    const resolved = await resolveSidecar(binary, async (info) => {
      attempts.push(info.downloadURL);
      if (attempts.length === 1) {
        throw new Error('pinned macOS asset no longer exists');
      }
      return info;
    });

    assert.equal(resolveCalls, 1);
    assert.deepEqual(attempts, [
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.3-aarch64-apple-darwin.tar.gz',
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz',
    ]);
    assert.equal(resolved.version, '0.26.4-alpha+sha.abcdef0');
    assert.equal(
      resolved.downloadURL,
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz',
    );
    assert.equal(resolved.targetFile, binary.targetFile);
  },
);
