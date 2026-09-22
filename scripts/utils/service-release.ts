export interface ServiceReleaseAsset {
  name: string;
}

export interface ServiceReleaseMetadata {
  tag_name: string;
  draft: boolean;
  prerelease: boolean;
  assets: ServiceReleaseAsset[];
}

export const EXPECTED_SERVICE_ASSETS = [
  'chimera-service-aarch64-apple-darwin.tar.gz',
  'chimera-service-aarch64-apple-darwin.tar.gz.sha256',
  'chimera-service-aarch64-pc-windows-msvc.zip',
  'chimera-service-aarch64-pc-windows-msvc.zip.sha256',
  'chimera-service-aarch64-unknown-linux-gnu.tar.gz',
  'chimera-service-aarch64-unknown-linux-gnu.tar.gz.sha256',
  'chimera-service-aarch64-unknown-linux-musl.tar.gz',
  'chimera-service-aarch64-unknown-linux-musl.tar.gz.sha256',
  'chimera-service-i686-pc-windows-msvc.zip',
  'chimera-service-i686-pc-windows-msvc.zip.sha256',
  'chimera-service-i686-unknown-linux-gnu.tar.gz',
  'chimera-service-i686-unknown-linux-gnu.tar.gz.sha256',
  'chimera-service-i686-unknown-linux-musl.tar.gz',
  'chimera-service-i686-unknown-linux-musl.tar.gz.sha256',
  'chimera-service-x86_64-apple-darwin.tar.gz',
  'chimera-service-x86_64-apple-darwin.tar.gz.sha256',
  'chimera-service-x86_64-pc-windows-msvc.zip',
  'chimera-service-x86_64-pc-windows-msvc.zip.sha256',
  'chimera-service-x86_64-unknown-linux-gnu.tar.gz',
  'chimera-service-x86_64-unknown-linux-gnu.tar.gz.sha256',
  'chimera-service-x86_64-unknown-linux-musl.tar.gz',
  'chimera-service-x86_64-unknown-linux-musl.tar.gz.sha256',
] as const;

export function assertServiceReleaseCommit(
  pinnedCommit: string,
  releaseCommit: string,
): void {
  const pinned = pinnedCommit.trim().toLowerCase();
  const released = releaseCommit.trim().toLowerCase();
  if (!/^[0-9a-f]{40}$/.test(pinned)) {
    throw new Error(`invalid pinned Chimera Service commit: ${pinnedCommit}`);
  }
  if (!/^[0-9a-f]{40}$/.test(released)) {
    throw new Error(
      `invalid released Chimera Service commit: ${releaseCommit}`,
    );
  }
  if (pinned !== released) {
    throw new Error(
      `Chimera Service release commit mismatch: pinned ${pinned}, released ${released}`,
    );
  }
}

export function assertStableServiceRelease(
  release: ServiceReleaseMetadata,
  expectedTag: string,
): void {
  if (release.tag_name !== expectedTag) {
    throw new Error(
      `service release tag mismatch: expected ${expectedTag}, got ${release.tag_name}`,
    );
  }
  if (release.draft) {
    throw new Error(`service release ${expectedTag} is still a draft`);
  }
  if (release.prerelease) {
    throw new Error(
      `service release ${expectedTag} is still a prerelease candidate`,
    );
  }

  const actual = new Set(release.assets.map((asset) => asset.name));
  const missing = EXPECTED_SERVICE_ASSETS.filter((name) => !actual.has(name));
  if (missing.length > 0) {
    throw new Error(
      `service release ${expectedTag} is missing assets: ${missing.join(', ')}`,
    );
  }
}
