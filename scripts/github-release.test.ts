import assert from 'node:assert/strict';
import test from 'node:test';
import {
  fetchReleaseAssetSignature,
  resolveReleaseByTag,
} from './github-release.ts';

test('downloads draft asset signatures through the authenticated API URL', async () => {
  const apiUrl =
    'https://api.github.com/repos/MFSGA/Chimera/releases/assets/123';
  const signature = await fetchReleaseAssetSignature(
    apiUrl,
    'test-token',
    async (input, init) => {
      assert.equal(input, apiUrl);
      assert.equal(init?.method, 'GET');
      assert.deepEqual(init?.headers, {
        Accept: 'application/octet-stream',
        Authorization: 'Bearer test-token',
      });
      return new Response('signature-content', { status: 200 });
    },
  );

  assert.equal(signature, 'signature-content');
});

test('reports unsuccessful draft asset signature responses', async () => {
  await assert.rejects(
    fetchReleaseAssetSignature(
      'https://api.github.com/repos/MFSGA/Chimera/releases/assets/123',
      'test-token',
      async () => new Response(null, { status: 404 }),
    ),
    /failed to fetch signature: 404/,
  );
});

test('uses the direct release lookup when the tag is indexed', async () => {
  const release = { tag_name: 'v0.27.1', id: 1 };
  let listed = false;

  const result = await resolveReleaseByTag(
    'v0.27.1',
    async () => release,
    async () => {
      listed = true;
      return [];
    },
  );

  assert.equal(result, release);
  assert.equal(listed, false);
});

test('finds a draft release in the releases list when tag lookup returns 404', async () => {
  const release = { tag_name: 'v0.27.1', draft: true, id: 1 };
  const result = await resolveReleaseByTag(
    'v0.27.1',
    async () => {
      throw Object.assign(new Error('Not Found'), { status: 404 });
    },
    async () => [{ tag_name: 'v0.27.0', id: 0 }, release],
  );

  assert.equal(result, release);
});

test('rethrows a 404 when no release has the exact requested tag', async () => {
  const error = Object.assign(new Error('Not Found'), { status: 404 });

  await assert.rejects(
    resolveReleaseByTag(
      'v0.27.1',
      async () => {
        throw error;
      },
      async () => [{ tag_name: 'v0.27.10' }],
    ),
    error,
  );
});

test('does not hide errors other than a missing release', async () => {
  const error = Object.assign(new Error('Forbidden'), { status: 403 });
  let listed = false;

  await assert.rejects(
    resolveReleaseByTag(
      'v0.27.1',
      async () => {
        throw error;
      },
      async () => {
        listed = true;
        return [];
      },
    ),
    error,
  );
  assert.equal(listed, false);
});
