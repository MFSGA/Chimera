export type UpdaterPlatform = {
  signature: string;
  url: string;
};

export type UpdaterManifest = {
  version: string;
  notes?: string | null;
  pub_date?: string;
  platforms: Record<string, UpdaterPlatform>;
};

export type StableUpdaterTarget =
  | 'win64'
  | 'windows-x86_64'
  | 'darwin'
  | 'darwin-intel'
  | 'darwin-x86_64'
  | 'darwin-aarch64';

export type StableUpdaterAssetMatch = {
  kind: 'url' | 'signature';
  targets: readonly StableUpdaterTarget[];
};

export function matchStableUpdaterAsset(
  name: string,
  fixedWebview = false,
): StableUpdaterAssetMatch | null {
  if (
    fixedWebview
      ? !name.includes('fixed-webview')
      : name.includes('fixed-webview')
  ) {
    return null;
  }

  if (name.endsWith('.exe') && name.includes('x64')) {
    return { kind: 'url', targets: ['win64', 'windows-x86_64'] };
  }
  if (name.endsWith('.exe.sig') && name.includes('x64')) {
    return { kind: 'signature', targets: ['win64', 'windows-x86_64'] };
  }
  if (name.endsWith('aarch64.app.tar.gz')) {
    return { kind: 'url', targets: ['darwin-aarch64'] };
  }
  if (name.endsWith('aarch64.app.tar.gz.sig')) {
    return { kind: 'signature', targets: ['darwin-aarch64'] };
  }
  if (name.endsWith('.app.tar.gz') && !name.includes('aarch')) {
    return {
      kind: 'url',
      targets: ['darwin', 'darwin-intel', 'darwin-x86_64'],
    };
  }
  if (name.endsWith('.app.tar.gz.sig') && !name.includes('aarch')) {
    return {
      kind: 'signature',
      targets: ['darwin', 'darwin-intel', 'darwin-x86_64'],
    };
  }

  return null;
}

const SEMVER_PATTERN =
  /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

export function assertUpdaterManifest(
  manifest: UpdaterManifest,
  requiredTargets: readonly string[] = ['windows-x86_64'],
) {
  if (typeof manifest.version !== 'string' || !manifest.version.trim()) {
    throw new Error('missing updater version');
  }

  const normalizedVersion = manifest.version.replace(/^v/, '');
  if (!SEMVER_PATTERN.test(normalizedVersion)) {
    throw new Error(`invalid updater version: ${manifest.version}`);
  }

  if (!manifest.platforms || typeof manifest.platforms !== 'object') {
    throw new Error('missing updater platforms');
  }

  if (manifest.pub_date && Number.isNaN(Date.parse(manifest.pub_date))) {
    throw new Error(`invalid updater pub_date: ${manifest.pub_date}`);
  }

  for (const target of requiredTargets) {
    if (!manifest.platforms[target]) {
      throw new Error(`missing updater platform: ${target}`);
    }
  }

  for (const [target, platform] of Object.entries(manifest.platforms)) {
    if (!platform.signature.trim()) {
      throw new Error(`missing updater signature for ${target}`);
    }

    let url: URL;
    try {
      url = new URL(platform.url);
    } catch {
      throw new Error(`invalid updater URL for ${target}: ${platform.url}`);
    }

    if (url.protocol !== 'https:' && url.protocol !== 'http:') {
      throw new Error(
        `unsupported updater URL protocol for ${target}: ${url.protocol}`,
      );
    }
  }
}
