import { ArchMapping } from 'utils/manifest';

export interface BinInfo {
  name: string;
  version?: string;
  targetFile: string;
  exeFile: string;
  tmpFile: string;
  downloadURL: string;
}

export enum SupportedArch {
  WindowsX86_64 = 'windows-x86_64',
  WindowsArm64 = 'windows-arm64',
  LinuxAarch64 = 'linux-aarch64',
  LinuxAmd64 = 'linux-amd64',
  DarwinArm64 = 'darwin-arm64',
  DarwinX64 = 'darwin-x64',
}

export interface ClashManifest {
  URL_PREFIX: string;
  LATEST_DATE?: string;
  STORAGE_PREFIX?: string;
  BACKUP_URL_PREFIX?: string;
  BACKUP_LATEST_DATE?: string;
  VERSION?: string;
  VERSION_URL?: string;
  ARCH_MAPPING: ArchMapping;
}

export enum SupportedCore {
  Mihomo = 'mihomo',
  MihomoAlpha = 'mihomo_alpha',
  ClashRs = 'clash_rs',
  ChimeraClient = 'chimera_client',
  ChimeraClientAlpha = 'chimera_client_alpha',
  ClashRsAlpha = 'clash_rs_alpha',
  ClashPremium = 'clash_premium',
}

export interface ManifestVersion {
  manifest_version: number;
  latest: { [K in SupportedCore]: string };
  arch_template: { [K in SupportedCore]: ArchMapping };
  updated_at: string; // ISO 8601
}
