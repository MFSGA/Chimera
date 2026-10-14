export type SupportedArch =
  | 'windows-i386'
  | 'windows-x86_64'
  | 'windows-arm64'
  | 'linux-aarch64'
  | 'linux-amd64'
  | 'linux-i386'
  | 'linux-armv7'
  | 'linux-armv7hf'
  | 'darwin-arm64'
  | 'darwin-x64';

export type ArchMapping = Record<SupportedArch, string>;

export interface BinInfo {
  name: string;
  version?: string;
  targetFile: string;
  exeFile: string;
  tmpFile: string;
  downloadURL: string;
  checksumURL?: string;
  refresh?: () => Promise<BinInfo>;
}

export interface VersionManifest {
  manifest_version: number;
  latest: {
    mihomo: string;
    mihomo_alpha: string;
    clash_rs: string;
    chimera_client: string;
    chimera_client_alpha: string;
    clash_premium: string;
    clash_rs_alpha: string;
  };
  arch_template: {
    mihomo: ArchMapping;
    mihomo_alpha: ArchMapping;
    clash_rs: ArchMapping;
    chimera_client: ArchMapping;
    chimera_client_alpha: ArchMapping;
    clash_premium: ArchMapping;
    clash_rs_alpha: ArchMapping;
  };
  updated_at: string;
}
