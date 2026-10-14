import type { BinInfo, SupportedArch, VersionManifest } from './types.ts';

type LatestChimeraClientAlpha = {
  version: string;
  archMapping: VersionManifest['arch_template']['chimera_client_alpha'];
};

export interface ChimeraClientAlphaResolverOptions {
  versionManifest: VersionManifest;
  platform: string;
  arch: SupportedArch;
  sidecarHost: string;
  resolveLatest?: () => Promise<LatestChimeraClientAlpha>;
}

function stableVersionFromAlpha(version: string): string {
  const match = version.match(/^(.+)-alpha\+sha\.[0-9a-f]{7,40}$/i);
  if (!match) {
    throw new Error(`invalid Chimera Client Alpha version: ${version}`);
  }
  return match[1];
}

export function getChimeraClientAlphaInfo({
  versionManifest,
  platform,
  arch,
  sidecarHost,
  resolveLatest,
}: ChimeraClientAlphaResolverOptions): BinInfo {
  const makeInfo = (
    version: string,
    assetVersion: string,
    archMapping: VersionManifest['arch_template']['chimera_client_alpha'],
  ): BinInfo => {
    const name = archMapping[arch].replace('{}', assetVersion);
    const isWin = platform === 'win32';
    const exeFile = arch.startsWith('darwin-')
      ? `${name.replace(/\.tar\.gz$/, '')}/clash-rs`
      : name;

    return {
      name: 'chimera-client-alpha',
      version,
      targetFile: `chimera-client-alpha-${sidecarHost}${isWin ? '.exe' : ''}`,
      exeFile,
      tmpFile: name,
      downloadURL: `https://github.com/MFSGA/Chimera_Client/releases/download/latest/${name}`,
    };
  };

  const pinned = makeInfo(
    versionManifest.latest.chimera_client_alpha,
    versionManifest.latest.chimera_client.replace(/^v/, ''),
    versionManifest.arch_template.chimera_client_alpha,
  );

  return {
    ...pinned,
    refresh: async () => {
      const latest = resolveLatest
        ? await resolveLatest()
        : await (await import('../../manifest.ts')).resolveChimeraClientAlpha();
      return makeInfo(
        latest.version,
        stableVersionFromAlpha(latest.version),
        latest.archMapping,
      );
    },
  };
}
