// import { ArchMapping } from 'utils/manifest';
import { fetch, type RequestInit } from 'undici';
import { CLASH_META_MANIFEST } from '../manifest/clash-meta';
import { BinInfo, SupportedArch } from '../types';
import { getProxyAgent } from './';
import { SIDECAR_HOST } from './consts';
import { consola } from './logger';

type NodeArch = NodeJS.Architecture | 'armel';

function mappingArch(platform: NodeJS.Platform, arch: NodeArch): SupportedArch {
  const label = `${platform}-${arch}`;
  switch (label) {
    case 'darwin-x64':
      return SupportedArch.DarwinX64;
    case 'darwin-arm64':
      return SupportedArch.DarwinArm64;
    case 'win32-x64':
      return SupportedArch.WindowsX86_64;
    case 'win32-ia32':
      return SupportedArch.WindowsX86_32;
    case 'win32-arm64':
      return SupportedArch.WindowsArm64;
    case 'linux-x64':
      return SupportedArch.LinuxAmd64;
    case 'linux-ia32':
      return SupportedArch.LinuxI386;
    case 'linux-arm':
      return SupportedArch.LinuxArmv7hf;
    case 'linux-arm64':
      return SupportedArch.LinuxAarch64;
    case 'linux-armel':
      return SupportedArch.LinuxArmv7;
    default:
      throw new Error('Unsupported platform/architecture: ' + label);
  }
}

export const getClashMetaInfo = ({
  platform,
  arch,
  sidecarHost,
}: {
  platform: NodeJS.Platform;
  arch: NodeArch;
  sidecarHost?: string;
}): BinInfo => {
  const { ARCH_MAPPING, URL_PREFIX, VERSION } = CLASH_META_MANIFEST;
  const archLabel = mappingArch(platform, arch);

  const name = ARCH_MAPPING[archLabel].replace('{}', VERSION as string);

  const isWin = platform === 'win32';

  const downloadURL = `${URL_PREFIX}/${name}`;

  const exeFile = `${name}${isWin ? '.exe' : ''}`;

  const tmpFile = `${name}`;

  const targetFile = `mihomo-${sidecarHost}${isWin ? '.exe' : ''}`;

  return {
    name: 'mihomo',
    targetFile,
    exeFile,
    tmpFile,
    downloadURL,
  };
};
