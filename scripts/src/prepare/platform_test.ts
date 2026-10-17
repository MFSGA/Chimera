import { assertEquals, assertThrows } from 'jsr:@std/assert@1';
import { mapArch, normalizeArch, normalizePlatform } from './platform.ts';

Deno.test('normalizes Deno platform and architecture names', () => {
  assertEquals(normalizePlatform('windows'), 'win32');
  assertEquals(normalizePlatform('darwin'), 'darwin');
  assertEquals(normalizeArch('x86_64'), 'x64');
  assertEquals(normalizeArch('aarch64'), 'arm64');
  assertEquals(normalizeArch('ia32'), 'ia32');
});

Deno.test('maps every manifest platform and architecture pair', () => {
  const mappings = [
    ['win32', 'ia32', 'windows-i386'],
    ['win32', 'x64', 'windows-x86_64'],
    ['win32', 'arm64', 'windows-arm64'],
    ['linux', 'arm64', 'linux-aarch64'],
    ['linux', 'x64', 'linux-amd64'],
    ['linux', 'ia32', 'linux-i386'],
    ['linux', 'armel', 'linux-armv7'],
    ['linux', 'arm', 'linux-armv7hf'],
    ['darwin', 'arm64', 'darwin-arm64'],
    ['darwin', 'x64', 'darwin-x64'],
  ] as const;

  for (const [platform, arch, expected] of mappings) {
    assertEquals(mapArch(platform, arch), expected);
  }
});

Deno.test('rejects unsupported platform and architecture pairs', () => {
  assertThrows(
    () => mapArch('freebsd', 'x64'),
    Error,
    'Unsupported platform/architecture: freebsd-x64',
  );
});
