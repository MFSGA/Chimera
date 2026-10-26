import { assertEquals } from 'jsr:@std/assert@1';
import { resolveChimeraClientAlphaFromGithub } from './chimera-client-alpha.ts';

Deno.test(
  'resolves Chimera Client Alpha from the rolling latest release assets',
  async () => {
    const requestedUrls: string[] = [];
    const request = async <T>(url: string): Promise<T> => {
      requestedUrls.push(url);
      if (url.endsWith('/git/ref/tags/latest')) {
        return {
          object: {
            type: 'commit',
            sha: '2c354de1e626310de937a32bdfb8297ca548f634',
          },
        } as T;
      }
      if (url.endsWith('/releases/tags/latest')) {
        return {
          assets: [
            { name: 'chimera-client-0.28.0-aarch64-apple-darwin.tar.gz' },
            { name: 'chimera-client-0.28.0-x86_64-apple-darwin.tar.gz' },
          ],
        } as T;
      }
      throw new Error(`Unexpected GitHub API request: ${url}`);
    };

    const result = await resolveChimeraClientAlphaFromGithub(request);

    assertEquals(result.version, '0.28.0-alpha+sha.2c354de');
    assertEquals(
      result.archMapping['darwin-arm64'],
      'chimera-client-{}-aarch64-apple-darwin.tar.gz',
    );
    assertEquals(requestedUrls.sort(), [
      'https://api.github.com/repos/MFSGA/Chimera_Client/git/ref/tags/latest',
      'https://api.github.com/repos/MFSGA/Chimera_Client/releases/tags/latest',
    ]);
  },
);
