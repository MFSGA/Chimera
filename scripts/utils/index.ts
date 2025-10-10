import { ProxyAgent } from 'undici';

export const HTTP_PROXY =
  process.env.HTTP_PROXY ||
  process.env.http_proxy ||
  process.env.HTTPS_PROXY ||
  process.env.https_proxy;

export function getProxyAgent() {
  if (HTTP_PROXY) {
    return new ProxyAgent(HTTP_PROXY);
  }

  return undefined;
}

export enum SupportedArch {
  WindowsX86_32 = 'windows-i386',
  WindowsX86_64 = 'windows-x86_64',
  WindowsArm64 = 'windows-arm64',
  LinuxAarch64 = 'linux-aarch64',
  LinuxAmd64 = 'linux-amd64',
  LinuxI386 = 'linux-i386',
  LinuxArmv7 = 'linux-armv7',
  LinuxArmv7hf = 'linux-armv7hf',
  DarwinArm64 = 'darwin-arm64',
  DarwinX64 = 'darwin-x64',
}
