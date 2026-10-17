import { assertEquals } from 'jsr:@std/assert@1';
import * as path from 'jsr:@std/path';
import { createBinaryResolvers } from './binaries.ts';
import type { ArchMapping, SupportedArch, VersionManifest } from './types.ts';

const platforms: Array<{
  platform: string;
  arch: string;
  label: SupportedArch;
  host: string;
}> = [
  {
    platform: 'win32',
    arch: 'ia32',
    label: 'windows-i386',
    host: 'i686-pc-windows-msvc',
  },
  {
    platform: 'win32',
    arch: 'x64',
    label: 'windows-x86_64',
    host: 'x86_64-pc-windows-msvc',
  },
  {
    platform: 'win32',
    arch: 'arm64',
    label: 'windows-arm64',
    host: 'aarch64-pc-windows-msvc',
  },
  {
    platform: 'linux',
    arch: 'arm64',
    label: 'linux-aarch64',
    host: 'aarch64-unknown-linux-gnu',
  },
  {
    platform: 'linux',
    arch: 'x64',
    label: 'linux-amd64',
    host: 'x86_64-unknown-linux-gnu',
  },
  {
    platform: 'linux',
    arch: 'ia32',
    label: 'linux-i386',
    host: 'i686-unknown-linux-gnu',
  },
  {
    platform: 'linux',
    arch: 'arm',
    label: 'linux-armv7hf',
    host: 'armv7-unknown-linux-gnueabihf',
  },
  {
    platform: 'linux',
    arch: 'armel',
    label: 'linux-armv7',
    host: 'armv7-unknown-linux-gnueabi',
  },
  {
    platform: 'darwin',
    arch: 'arm64',
    label: 'darwin-arm64',
    host: 'aarch64-apple-darwin',
  },
  {
    platform: 'darwin',
    arch: 'x64',
    label: 'darwin-x64',
    host: 'x86_64-apple-darwin',
  },
];

const mapping = (values: Record<SupportedArch, string>): ArchMapping => values;

const versionManifest: VersionManifest = {
  manifest_version: 1,
  latest: {
    mihomo: 'v1.19.32',
    mihomo_alpha: 'alpha-94b13ae',
    clash_rs: 'v0.10.10',
    chimera_client: 'v0.26.3',
    chimera_client_alpha: '0.26.3-alpha+sha.406b0e0',
    clash_premium: '2023-09-05-gdcc8d87',
    clash_rs_alpha: '0.10.10-alpha+sha.36eec77',
  },
  arch_template: {
    mihomo: mapping({
      'windows-i386': 'mihomo-windows-386-{}.zip',
      'windows-x86_64': 'mihomo-windows-amd64-v1-{}.zip',
      'windows-arm64': 'mihomo-windows-arm64-{}.zip',
      'linux-aarch64': 'mihomo-linux-arm64-{}.gz',
      'linux-amd64': 'mihomo-linux-amd64-v1-{}.gz',
      'linux-i386': 'mihomo-linux-386-{}.gz',
      'linux-armv7': 'mihomo-linux-armv5-{}.gz',
      'linux-armv7hf': 'mihomo-linux-armv7-{}.gz',
      'darwin-arm64': 'mihomo-darwin-arm64-{}.gz',
      'darwin-x64': 'mihomo-darwin-amd64-v1-{}.gz',
    }),
    mihomo_alpha: mapping({
      'windows-i386': 'mihomo-windows-386-{}.zip',
      'windows-x86_64': 'mihomo-windows-amd64-v1-{}.zip',
      'windows-arm64': 'mihomo-windows-arm64-{}.zip',
      'linux-aarch64': 'mihomo-linux-arm64-{}.gz',
      'linux-amd64': 'mihomo-linux-amd64-v1-{}.gz',
      'linux-i386': 'mihomo-linux-386-{}.gz',
      'linux-armv7': 'mihomo-linux-armv5-{}.gz',
      'linux-armv7hf': 'mihomo-linux-armv7-{}.gz',
      'darwin-arm64': 'mihomo-darwin-arm64-{}.gz',
      'darwin-x64': 'mihomo-darwin-amd64-v1-{}.gz',
    }),
    clash_rs: mapping({
      'windows-i386': 'clash-rs-i686-pc-windows-msvc-static-crt.exe',
      'windows-x86_64': 'clash-rs-x86_64-pc-windows-msvc.exe',
      'windows-arm64': 'clash-rs-aarch64-pc-windows-msvc.exe',
      'linux-aarch64': 'clash-rs-aarch64-unknown-linux-gnu',
      'linux-amd64': 'clash-rs-x86_64-unknown-linux-gnu-static-crt',
      'linux-i386': 'clash-rs-i686-unknown-linux-gnu',
      'linux-armv7': 'clash-rs-armv7-unknown-linux-gnueabi',
      'linux-armv7hf': 'clash-rs-armv7-unknown-linux-gnueabihf',
      'darwin-arm64': 'clash-rs-aarch64-apple-darwin',
      'darwin-x64': 'clash-rs-x86_64-apple-darwin',
    }),
    chimera_client: mapping({
      'windows-i386': 'clash_chimera-i686-pc-windows-msvc.exe',
      'windows-x86_64': 'clash_chimera-x86_64-pc-windows-msvc.exe',
      'windows-arm64': 'clash_chimera-aarch64-pc-windows-msvc.exe',
      'linux-aarch64': 'clash_chimera-aarch64-unknown-linux-gnu',
      'linux-amd64': 'clash_chimera-x86_64-unknown-linux-gnu',
      'linux-i386': 'clash_chimera-i686-unknown-linux-musl',
      'linux-armv7': 'clash_chimera-armv7-unknown-linux-gnueabi',
      'linux-armv7hf': 'clash_chimera-armv7-unknown-linux-gnueabihf',
      'darwin-arm64': 'chimera-client-{}-aarch64-apple-darwin.tar.gz',
      'darwin-x64': 'chimera-client-{}-x86_64-apple-darwin.tar.gz',
    }),
    chimera_client_alpha: mapping({
      'windows-i386': 'clash_chimera-i686-pc-windows-msvc.exe',
      'windows-x86_64': 'clash_chimera-x86_64-pc-windows-msvc.exe',
      'windows-arm64': 'clash_chimera-aarch64-pc-windows-msvc.exe',
      'linux-aarch64': 'clash_chimera-aarch64-unknown-linux-gnu',
      'linux-amd64': 'clash_chimera-x86_64-unknown-linux-gnu',
      'linux-i386': 'clash_chimera-i686-unknown-linux-musl',
      'linux-armv7': 'clash_chimera-armv7-unknown-linux-gnueabi',
      'linux-armv7hf': 'clash_chimera-armv7-unknown-linux-gnueabihf',
      'darwin-arm64': 'chimera-client-{}-aarch64-apple-darwin.tar.gz',
      'darwin-x64': 'chimera-client-{}-x86_64-apple-darwin.tar.gz',
    }),
    clash_premium: mapping({
      'windows-i386': 'clash-windows-386-n{}.zip',
      'windows-x86_64': 'clash-windows-amd64-n{}.zip',
      'windows-arm64': 'clash-windows-arm64-n{}.zip',
      'linux-aarch64': 'clash-linux-arm64-n{}.gz',
      'linux-amd64': 'clash-linux-amd64-n{}.gz',
      'linux-i386': 'clash-linux-386-n{}.gz',
      'linux-armv7': 'clash-linux-armv5-n{}.gz',
      'linux-armv7hf': 'clash-linux-armv7-n{}.gz',
      'darwin-arm64': 'clash-darwin-arm64-n{}.gz',
      'darwin-x64': 'clash-darwin-amd64-n{}.gz',
    }),
    clash_rs_alpha: mapping({
      'windows-i386': 'clash-rs-i686-pc-windows-msvc-static-crt.exe',
      'windows-x86_64': 'clash-rs-x86_64-pc-windows-msvc.exe',
      'windows-arm64': 'clash-rs-aarch64-pc-windows-msvc.exe',
      'linux-aarch64': 'clash-rs-aarch64-unknown-linux-gnu',
      'linux-amd64': 'clash-rs-x86_64-unknown-linux-gnu-static-crt',
      'linux-i386': 'clash-rs-i686-unknown-linux-gnu',
      'linux-armv7': 'clash-rs-armv7-unknown-linux-gnueabi',
      'linux-armv7hf': 'clash-rs-armv7-unknown-linux-gnueabihf',
      'darwin-arm64': 'clash-rs-aarch64-apple-darwin',
      'darwin-x64': 'clash-rs-x86_64-apple-darwin',
    }),
  },
  updated_at: '2026-10-14T00:00:00Z',
};

function createResolvers(
  platform: string,
  arch: string,
  sidecarHost: string,
  workspaceRoot = '/workspace',
  resolveLatestChimeraClientAlpha?: () => Promise<{
    version: string;
    archMapping: ArchMapping;
  }>,
) {
  return createBinaryResolvers({
    versionManifest,
    platform,
    arch,
    sidecarHost,
    workspaceRoot,
    resolveLatestChimeraClientAlpha,
  });
}

Deno.test(
  'binary resolvers preserve all supported architecture mappings',
  () => {
    for (const { platform, arch, label, host } of platforms) {
      const binary = createResolvers(platform, arch, host);

      const clash = binary.clash();
      const clashAsset = versionManifest.arch_template.clash_premium[
        label
      ].replace('{}', versionManifest.latest.clash_premium);
      assertEquals(
        clash.targetFile,
        `clash-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(
        clash.exeFile,
        `${clashAsset}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(
        clash.downloadURL,
        `https://github.com/zhongfly/Clash-premium-backup/releases/download/${versionManifest.latest.clash_premium}/${clashAsset}`,
      );

      const mihomo = binary.mihomo();
      const mihomoAsset = versionManifest.arch_template.mihomo[label].replace(
        '{}',
        versionManifest.latest.mihomo,
      );
      assertEquals(
        mihomo.targetFile,
        `mihomo-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(mihomo.tmpFile, mihomoAsset);
      assertEquals(
        mihomo.downloadURL,
        `https://github.com/MetaCubeX/mihomo/releases/download/${versionManifest.latest.mihomo}/${mihomoAsset}`,
      );

      const clashRs = binary.clashRs();
      const clashRsAsset = versionManifest.arch_template.clash_rs[label];
      assertEquals(
        clashRs.targetFile,
        `clash-rs-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(clashRs.tmpFile, clashRsAsset);
      assertEquals(
        clashRs.downloadURL,
        `https://github.com/ibigbug/clash-rs/releases/download/${versionManifest.latest.clash_rs}/${clashRsAsset}`,
      );

      const clashRsAlpha = binary.clashRsAlpha();
      const clashRsAlphaAsset =
        versionManifest.arch_template.clash_rs_alpha[label];
      assertEquals(
        clashRsAlpha.targetFile,
        `clash-rs-alpha-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(clashRsAlpha.tmpFile, clashRsAlphaAsset);
      assertEquals(
        clashRsAlpha.downloadURL,
        `https://github.com/ibigbug/clash-rs/releases/download/latest/${clashRsAlphaAsset}`,
      );

      const client = binary.chimeraClient();
      const clientAsset = versionManifest.arch_template.chimera_client[
        label
      ].replace('{}', '0.26.3');
      assertEquals(
        client.targetFile,
        `chimera-client-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(client.tmpFile, clientAsset);
      assertEquals(
        client.downloadURL,
        `https://github.com/MFSGA/Chimera_Client/releases/download/v0.26.3/${clientAsset}`,
      );
      if (platform === 'darwin') {
        assertEquals(
          client.exeFile,
          `${clientAsset.replace(/\.tar\.gz$/, '')}/clash-rs`,
        );
      }

      const clientAlpha = binary.chimeraClientAlpha();
      const clientAlphaAsset =
        versionManifest.arch_template.chimera_client_alpha[label].replace(
          '{}',
          '0.26.3',
        );
      assertEquals(
        clientAlpha.targetFile,
        `chimera-client-alpha-${host}${platform === 'win32' ? '.exe' : ''}`,
      );
      assertEquals(clientAlpha.tmpFile, clientAlphaAsset);
      assertEquals(
        clientAlpha.downloadURL,
        `https://github.com/MFSGA/Chimera_Client/releases/download/latest/${clientAlphaAsset}`,
      );
    }
  },
);

Deno.test(
  'Mihomo Alpha resolves and refreshes metadata for every supported architecture',
  async () => {
    const originalFetch = globalThis.fetch;
    const requested: string[] = [];
    let version = 'alpha-1234567';
    let currentPlatform = platforms[0];
    globalThis.fetch = (input) => {
      const url = String(input);
      requested.push(url);
      const platformInfo = currentPlatform!;
      const assetTemplate =
        versionManifest.arch_template.mihomo_alpha[platformInfo.label];
      return Promise.resolve(
        new Response(
          url.endsWith('version.txt')
            ? version
            : assetTemplate.replace('{}', version),
        ),
      );
    };

    try {
      for (const platformInfo of platforms) {
        currentPlatform = platformInfo;
        const binary = createResolvers(
          platformInfo.platform,
          platformInfo.arch,
          platformInfo.host,
        );
        const first = await binary.mihomoAlpha();
        const assetTemplate =
          versionManifest.arch_template.mihomo_alpha[platformInfo.label];
        const initialAsset = assetTemplate.replace('{}', 'alpha-1234567');
        assertEquals(first.version, 'alpha-1234567');
        assertEquals(
          first.targetFile,
          `mihomo-alpha-${platformInfo.host}${platformInfo.platform === 'win32' ? '.exe' : ''}`,
        );
        assertEquals(first.tmpFile, initialAsset);
        assertEquals(
          first.exeFile,
          `${initialAsset}${platformInfo.platform === 'win32' ? '.exe' : ''}`,
        );
        assertEquals(
          first.downloadURL,
          `https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/${initialAsset}`,
        );

        version = 'alpha-7654321';
        const refreshed = await first.refresh!();
        const refreshedAsset = assetTemplate.replace('{}', 'alpha-7654321');
        assertEquals(refreshed.version, 'alpha-7654321');
        assertEquals(refreshed.tmpFile, refreshedAsset);
        assertEquals(
          refreshed.downloadURL,
          `https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/${refreshedAsset}`,
        );
        version = 'alpha-1234567';
      }
      assertEquals(requested.length, platforms.length * 4);
    } finally {
      globalThis.fetch = originalFetch;
    }
  },
);

Deno.test(
  'Mihomo Alpha falls back to the pinned manifest when metadata requests fail',
  async () => {
    const requested: string[] = [];
    const binary = createBinaryResolvers({
      versionManifest,
      platform: 'darwin',
      arch: 'arm64',
      sidecarHost: 'aarch64-apple-darwin',
      workspaceRoot: '/workspace',
      fetchText: async (url) => {
        requested.push(url);
        throw new TypeError('network unavailable');
      },
    });

    const resolved = await binary.mihomoAlpha();

    assertEquals(requested, [
      'https://github.com/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt',
      'https://github.com/MetaCubeX/mihomo/releases/expanded_assets/Prerelease-Alpha',
    ]);
    assertEquals(resolved.version, versionManifest.latest.mihomo_alpha);
    assertEquals(
      resolved.tmpFile,
      versionManifest.arch_template.mihomo_alpha['darwin-arm64'].replace(
        '{}',
        versionManifest.latest.mihomo_alpha,
      ),
    );
  },
);

Deno.test(
  'Chimera Client Alpha refreshes metadata for the selected platform',
  async () => {
    let resolveCalls = 0;
    const binary = createResolvers(
      'darwin',
      'arm64',
      'aarch64-apple-darwin',
      '/workspace',
      async () => {
        resolveCalls++;
        return {
          version: '0.26.4-alpha+sha.abcdef0',
          archMapping: versionManifest.arch_template.chimera_client_alpha,
        };
      },
    );
    const pinned = binary.chimeraClientAlpha();
    assertEquals(pinned.version, '0.26.3-alpha+sha.406b0e0');
    assertEquals(
      pinned.downloadURL,
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.3-aarch64-apple-darwin.tar.gz',
    );
    const refreshed = await pinned.refresh!();
    assertEquals(resolveCalls, 1);
    assertEquals(refreshed.version, '0.26.4-alpha+sha.abcdef0');
    assertEquals(
      refreshed.downloadURL,
      'https://github.com/MFSGA/Chimera_Client/releases/download/latest/' +
        'chimera-client-0.26.4-aarch64-apple-darwin.tar.gz',
    );
    assertEquals(refreshed.targetFile, pinned.targetFile);
  },
);

Deno.test(
  'Chimera Service sidecar version and checksum follow the local crate',
  async () => {
    const cases = [
      {
        platform: 'win32',
        arch: 'x64',
        host: 'x86_64-pc-windows-msvc',
        archive: 'zip',
        target: 'chimera-service-x86_64-pc-windows-msvc.exe',
        executable: 'chimera-service.exe',
      },
      {
        platform: 'darwin',
        arch: 'arm64',
        host: 'aarch64-apple-darwin',
        archive: 'tar.gz',
        target: 'chimera-service-aarch64-apple-darwin',
        executable: 'chimera-service',
      },
    ];
    for (const item of cases) {
      const binary = createBinaryResolvers({
        versionManifest,
        platform: item.platform,
        arch: item.arch,
        sidecarHost: item.host,
        workspaceRoot: '/workspace',
        readChimeraServiceManifest: async (manifestPath) => {
          assertEquals(
            manifestPath,
            path.join(
              '/workspace',
              'backend/chimera-runtime/chimera_service/Cargo.toml',
            ),
          );
          return '[package]\nname = "chimera-service"\nversion = "2.3.4-rc.1"\n';
        },
      });
      const service = await binary.chimeraService();
      const archiveName = `chimera-service-${item.host}.${item.archive}`;
      const releaseURL = `https://github.com/MFSGA/Chimera_Service/releases/download/v2.3.4-rc.1/${archiveName}`;

      assertEquals(service.version, 'v2.3.4-rc.1');
      assertEquals(service.targetFile, item.target);
      assertEquals(service.exeFile, item.executable);
      assertEquals(
        service.tmpFile,
        `chimera-service-v2.3.4-rc.1-${item.host}.${item.archive}`,
      );
      assertEquals(service.downloadURL, releaseURL);
      assertEquals(service.checksumURL, `${releaseURL}.sha256`);
    }
  },
);
