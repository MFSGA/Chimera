import type { ArchMapping } from './types.ts';
import { resolveAlphaVersionFromAssets } from './versions.ts';

type GithubFetch = <T>(url: string) => Promise<T>;

const OWNER = 'MFSGA';
const REPO = 'Chimera_Client';

export const CHIMERA_CLIENT_ALPHA_ARCH_MAPPING: ArchMapping = {
  'windows-i386': 'clash_chimera-i686-pc-windows-msvc.exe',
  'windows-x86_64': 'clash_chimera-x86_64-pc-windows-msvc.exe',
  'windows-arm64': 'clash_chimera-aarch64-pc-windows-msvc.exe',
  'linux-aarch64': 'clash_chimera-aarch64-unknown-linux-gnu',
  'linux-amd64': 'clash_chimera-x86_64-unknown-linux-gnu',
  'linux-i386': 'clash_chimera-i686-unknown-linux-musl',
  'darwin-arm64': 'chimera-client-{}-aarch64-apple-darwin.tar.gz',
  'darwin-x64': 'chimera-client-{}-x86_64-apple-darwin.tar.gz',
  'linux-armv7': 'clash_chimera-armv7-unknown-linux-gnueabi',
  'linux-armv7hf': 'clash_chimera-armv7-unknown-linux-gnueabihf',
};

export async function resolveChimeraClientAlphaFromGithub(
  request: GithubFetch,
): Promise<{ version: string; archMapping: ArchMapping }> {
  const baseUrl = `https://api.github.com/repos/${OWNER}/${REPO}`;
  const [ref, alphaRelease] = await Promise.all([
    request<{ object: { type: string; sha: string } }>(
      `${baseUrl}/git/ref/tags/latest`,
    ),
    request<{ assets: Array<{ name: string }> }>(
      `${baseUrl}/releases/tags/latest`,
    ),
  ]);

  let commitSha = ref.object.sha;
  if (ref.object.type === 'tag') {
    const tag = await request<{ object: { sha: string } }>(
      `${baseUrl}/git/tags/${commitSha}`,
    );
    commitSha = tag.object.sha;
  }

  const version = resolveAlphaVersionFromAssets(
    alphaRelease.assets.map(({ name }) => name),
    CHIMERA_CLIENT_ALPHA_ARCH_MAPPING['darwin-arm64'],
    commitSha,
  );
  if (!version) {
    throw new Error(
      'Unable to resolve Chimera Client Alpha version from latest release assets',
    );
  }

  return { version, archMapping: CHIMERA_CLIENT_ALPHA_ARCH_MAPPING };
}
