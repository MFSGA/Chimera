import * as path from 'jsr:@std/path';
import { mapArch } from './platform.ts';
import type {
  BinInfo,
  ClashManifest,
  SupportedArch,
  VersionManifest,
} from './types.ts';
import {
  extractVersionFromAssetList,
  fetchText,
  isValidVersion,
  normalizeVersion,
} from './versions.ts';

const MIHOMO_ALPHA_ASSETS_URL =
  'https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha';

type DebugLog = (message: string) => void;

type LatestChimeraClientAlpha = {
  version: string;
  archMapping: VersionManifest['arch_template']['chimera_client_alpha'];
};

export interface BinaryResolverOptions {
  versionManifest: VersionManifest;
  sidecarHost: string;
  platform: string;
  arch: string;
  workspaceRoot: string;
  debugLog?: DebugLog;
  fetchText?: typeof fetchText;
  resolveLatestChimeraClientAlpha?: () => Promise<LatestChimeraClientAlpha>;
  readChimeraServiceManifest?: (path: string) => Promise<string>;
}

function stableVersionFromAlpha(version: string): string {
  const match = version.match(/^(.+)-alpha\+sha\.[0-9a-f]{7,40}$/i);
  if (!match) {
    throw new Error(`invalid Chimera Client Alpha version: ${version}`);
  }
  return match[1];
}

export function createBinaryResolvers(options: BinaryResolverOptions) {
  const { versionManifest, sidecarHost, platform, arch, workspaceRoot } =
    options;
  const debugLog = options.debugLog ?? (() => {});
  const requestText = options.fetchText ?? fetchText;

  const clashManifest: ClashManifest = {
    URL_PREFIX: 'https://github.com/Dreamacro/clash/releases/download/premium/',
    BACKUP_URL_PREFIX:
      'https://github.com/zhongfly/Clash-premium-backup/releases/download/',
    BACKUP_LATEST_DATE: versionManifest.latest.clash_premium,
    VERSION: versionManifest.latest.clash_premium,
    ARCH_MAPPING: versionManifest.arch_template.clash_premium,
  };

  const clashMetaManifest: ClashManifest = {
    URL_PREFIX: `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}`,
    VERSION: versionManifest.latest.mihomo,
    ARCH_MAPPING: versionManifest.arch_template.mihomo,
  };

  const clashMetaAlphaManifest: ClashManifest = {
    VERSION_URL:
      'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt',
    URL_PREFIX:
      'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha',
    ARCH_MAPPING: versionManifest.arch_template.mihomo_alpha,
  };

  const clashRsManifest: ClashManifest = {
    URL_PREFIX: 'https://github.com/ibigbug/clash-rs/releases/download/',
    VERSION: versionManifest.latest.clash_rs,
    ARCH_MAPPING: versionManifest.arch_template.clash_rs,
  };

  const clashRsAlphaManifest: ClashManifest = {
    // The rolling latest tag serves versionless assets, so its URL stays valid
    // when the manifest version lags behind.
    URL_PREFIX: 'https://github.com/ibigbug/clash-rs/releases/download/latest',
    ARCH_MAPPING: versionManifest.arch_template.clash_rs_alpha,
  };

  const chimeraClientManifest: ClashManifest = {
    URL_PREFIX: 'https://github.com/MFSGA/Chimera_Client/releases/download/',
    VERSION: versionManifest.latest.chimera_client,
    ARCH_MAPPING: versionManifest.arch_template.chimera_client,
  };

  const chimeraClientAlphaManifest: ClashManifest = {
    URL_PREFIX:
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest',
    VERSION: versionManifest.latest.chimera_client_alpha,
    ARCH_MAPPING: versionManifest.arch_template.chimera_client_alpha,
  };

  function targetName(name: string): string {
    return `${name}-${sidecarHost}${platform === 'win32' ? '.exe' : ''}`;
  }

  function withMappedAsset(manifest: ClashManifest): {
    version?: string;
    assetName: string;
  } {
    const version = manifest.VERSION ?? manifest.BACKUP_LATEST_DATE;
    const assetName = manifest.ARCH_MAPPING[mapArch(platform, arch)].replace(
      '{}',
      version!,
    );
    return { version, assetName };
  }

  function getClashBackupInfo(): BinInfo {
    const { BACKUP_LATEST_DATE, BACKUP_URL_PREFIX } = clashManifest;
    const { assetName } = withMappedAsset(clashManifest);
    const name = assetName;
    return {
      name: 'clash',
      version: BACKUP_LATEST_DATE,
      targetFile: targetName('clash'),
      exeFile: `${name}${platform === 'win32' ? '.exe' : ''}`,
      tmpFile: name,
      downloadURL: `${BACKUP_URL_PREFIX}${BACKUP_LATEST_DATE}/${name}`,
    };
  }

  function getClashMetaInfo(): BinInfo {
    const { URL_PREFIX } = clashMetaManifest;
    const { version, assetName } = withMappedAsset(clashMetaManifest);
    return {
      name: 'mihomo',
      version,
      targetFile: targetName('mihomo'),
      exeFile: `${assetName}${platform === 'win32' ? '.exe' : ''}`,
      tmpFile: assetName,
      downloadURL: `${URL_PREFIX}/${assetName}`,
    };
  }

  async function fetchAlphaText(url: string): Promise<string | undefined> {
    try {
      return await requestText(url);
    } catch (error) {
      debugLog(
        `failed to fetch mihomo-alpha metadata from ${url}: ${String(error)}`,
      );
      return undefined;
    }
  }

  async function getClashMetaAlphaInfo(): Promise<BinInfo> {
    const { ARCH_MAPPING, URL_PREFIX, VERSION_URL } = clashMetaAlphaManifest;
    const archLabel = mapArch(platform, arch);
    const assetTemplate = ARCH_MAPPING[archLabel];
    const versionFromFile = normalizeVersion(
      (await fetchAlphaText(VERSION_URL!))?.trim(),
    );
    const versionFromAssets = extractVersionFromAssetList(
      (await fetchAlphaText(MIHOMO_ALPHA_ASSETS_URL)) ?? '',
      assetTemplate,
    );
    const fallbackVersion = normalizeVersion(
      versionManifest.latest.mihomo_alpha,
    );
    const version = [versionFromFile, versionFromAssets, fallbackVersion].find(
      isValidVersion,
    );
    if (!version) throw new Error('cannot resolve mihomo-alpha version');

    debugLog(`mihomo-alpha version: ${version}`);
    const name = assetTemplate.replace('{}', version);
    return {
      name: 'mihomo-alpha',
      version,
      targetFile: targetName('mihomo-alpha'),
      exeFile: `${name}${platform === 'win32' ? '.exe' : ''}`,
      tmpFile: name,
      downloadURL: `${URL_PREFIX}/${name}`,
      refresh: () => getClashMetaAlphaInfo(),
    };
  }

  function getClashRustInfo(): BinInfo {
    const { URL_PREFIX, VERSION } = clashRsManifest;
    const { assetName } = withMappedAsset(clashRsManifest);
    return {
      name: 'clash-rs',
      version: VERSION,
      targetFile: targetName('clash-rs'),
      exeFile: assetName,
      tmpFile: assetName,
      downloadURL: `${URL_PREFIX}${VERSION}/${assetName}`,
    };
  }

  function getClashRustAlphaInfo(): BinInfo {
    const { ARCH_MAPPING, URL_PREFIX } = clashRsAlphaManifest;
    const version = versionManifest.latest.clash_rs_alpha;
    debugLog(`clash-rs-alpha version: ${version}`);
    const name = ARCH_MAPPING[mapArch(platform, arch)].replace('{}', version);
    return {
      name: 'clash-rs-alpha',
      version,
      targetFile: targetName('clash-rs-alpha'),
      exeFile: name,
      tmpFile: name,
      downloadURL: `${URL_PREFIX}/${name}`,
    };
  }

  function getChimeraClientInfo(): BinInfo {
    const { ARCH_MAPPING, URL_PREFIX, VERSION } = chimeraClientManifest;
    const archLabel = mapArch(platform, arch);
    const assetVersion = VERSION!.replace(/^v/, '');
    const name = ARCH_MAPPING[archLabel].replace('{}', assetVersion);
    const exeFile = archLabel.startsWith('darwin-')
      ? `${name.replace(/\.tar\.gz$/, '')}/clash-rs`
      : name;
    return {
      name: 'chimera-client',
      version: VERSION,
      targetFile: targetName('chimera-client'),
      exeFile,
      tmpFile: name,
      downloadURL: `${URL_PREFIX}${VERSION!.startsWith('v') ? VERSION : `v${VERSION}`}/${name}`,
    };
  }

  function makeChimeraClientAlphaInfo(
    version: string,
    archMapping: VersionManifest['arch_template']['chimera_client_alpha'],
  ): BinInfo {
    const archLabel: SupportedArch = mapArch(platform, arch);
    const assetVersion = stableVersionFromAlpha(version);
    const name = archMapping[archLabel].replace('{}', assetVersion);
    const exeFile = archLabel.startsWith('darwin-')
      ? `${name.replace(/\.tar\.gz$/, '')}/clash-rs`
      : name;
    return {
      name: 'chimera-client-alpha',
      version,
      targetFile: targetName('chimera-client-alpha'),
      exeFile,
      tmpFile: name,
      downloadURL: `${chimeraClientAlphaManifest.URL_PREFIX}/${name}`,
    };
  }

  function getChimeraClientAlphaInfo(): BinInfo {
    const pinned = makeChimeraClientAlphaInfo(
      chimeraClientAlphaManifest.VERSION!,
      chimeraClientAlphaManifest.ARCH_MAPPING,
    );
    return {
      ...pinned,
      refresh: async () => {
        const latest = options.resolveLatestChimeraClientAlpha
          ? await options.resolveLatestChimeraClientAlpha()
          : await (
              await import('../../manifest.ts')
            ).resolveChimeraClientAlpha();
        return makeChimeraClientAlphaInfo(latest.version, latest.archMapping);
      },
    };
  }

  async function getChimeraServiceInfo(): Promise<BinInfo> {
    const manifestPath = path.join(
      workspaceRoot,
      'backend/chimera-runtime/chimera_service/Cargo.toml',
    );
    const manifest = await (
      options.readChimeraServiceManifest ?? Deno.readTextFile
    )(manifestPath);
    const match = manifest.match(
      /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms,
    );
    if (!match) {
      throw new Error(
        `failed to parse chimera-service version from ${manifestPath}`,
      );
    }
    const version = `v${match[1]}`;
    debugLog(`chimera-service version: ${version}`);
    const serviceRepo = 'MFSGA/Chimera_Service';
    const isWin = sidecarHost.includes('windows');
    const urlExt = isWin ? 'zip' : 'tar.gz';
    const name = 'chimera-service';
    const archiveName = `${name}-${sidecarHost}.${urlExt}`;
    return {
      name,
      version,
      targetFile: `${name}-${sidecarHost}${isWin ? '.exe' : ''}`,
      exeFile: `${name}${isWin ? '.exe' : ''}`,
      tmpFile: `${name}-${version}-${sidecarHost}.${urlExt}`,
      downloadURL: `https://github.com/${serviceRepo}/releases/download/${version}/${archiveName}`,
      checksumURL: `https://github.com/${serviceRepo}/releases/download/${version}/${archiveName}.sha256`,
    };
  }

  return {
    clash: getClashBackupInfo,
    mihomo: getClashMetaInfo,
    mihomoAlpha: getClashMetaAlphaInfo,
    clashRs: getClashRustInfo,
    clashRsAlpha: getClashRustAlphaInfo,
    chimeraClient: getChimeraClientInfo,
    chimeraClientAlpha: getChimeraClientAlphaInfo,
    chimeraService: getChimeraServiceInfo,
  };
}
