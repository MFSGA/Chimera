import type { SupportedArch } from './types.ts';

const DENO_ARCH_TO_NODE: Record<string, string> = {
  x86_64: 'x64',
  aarch64: 'arm64',
};

export function normalizePlatform(platform: string): string {
  return platform === 'windows' ? 'win32' : platform;
}

export function normalizeArch(arch: string): string {
  return DENO_ARCH_TO_NODE[arch] ?? arch;
}

export function mapArch(platform: string, arch: string): SupportedArch {
  const mapping: Partial<Record<string, SupportedArch>> = {
    'darwin-x64': 'darwin-x64',
    'darwin-arm64': 'darwin-arm64',
    'win32-ia32': 'windows-i386',
    'win32-x64': 'windows-x86_64',
    'win32-arm64': 'windows-arm64',
    'linux-x64': 'linux-amd64',
    'linux-ia32': 'linux-i386',
    'linux-arm': 'linux-armv7hf',
    'linux-arm64': 'linux-aarch64',
    'linux-armel': 'linux-armv7',
  };
  const result = mapping[`${platform}-${arch}`];
  if (!result) {
    throw new Error(`Unsupported platform/architecture: ${platform}-${arch}`);
  }
  return result;
}
