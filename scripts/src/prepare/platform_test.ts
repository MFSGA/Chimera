import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mapArch, normalizeArch, normalizePlatform } from './platform.ts';

test('normalizes platform and architecture names', () => {
  assert.deepStrictEqual(normalizePlatform('windows'), 'win32');
  assert.deepStrictEqual(normalizePlatform('darwin'), 'darwin');
  assert.deepStrictEqual(normalizeArch('x86_64'), 'x64');
  assert.deepStrictEqual(normalizeArch('aarch64'), 'arm64');
  assert.deepStrictEqual(normalizeArch('ia32'), 'ia32');
});

test('maps every manifest platform and architecture pair', () => {
  const mappings = [
    ['win32', 'x64', 'windows-x86_64'],
    ['win32', 'arm64', 'windows-arm64'],
    ['linux', 'arm64', 'linux-aarch64'],
    ['linux', 'x64', 'linux-amd64'],
    ['darwin', 'arm64', 'darwin-arm64'],
    ['darwin', 'x64', 'darwin-x64'],
  ] as const;

  for (const [platform, arch, expected] of mappings) {
    assert.deepStrictEqual(mapArch(platform, arch), expected);
  }
});

test('rejects unsupported platform and architecture pairs', () => {
  for (const [platform, arch] of [
    ['freebsd', 'x64'],
    ['win32', 'ia32'],
    ['linux', 'ia32'],
    ['linux', 'arm'],
    ['linux', 'armel'],
  ] as const) {
    assert.throws(() => mapArch(platform, arch), {
      message: `Unsupported platform/architecture: ${platform}-${arch}`,
    });
  }
});
