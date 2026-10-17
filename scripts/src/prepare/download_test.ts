import assert from 'node:assert/strict';
import * as path from 'jsr:@std/path';
import { downloadFile } from './download.ts';
import { withTempFetch } from './test_utils.ts';

Deno.test({
  name: 'downloadFile writes streamed chunks and reports bounded progress',
  timeout: 5_000,
  fn: async () =>
    withTempFetch(async (root) => {
      const filePath = path.join(root, 'asset.bin');
      globalThis.fetch = (() =>
        Promise.resolve(
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.enqueue(new Uint8Array([1, 2]));
                controller.enqueue(new Uint8Array([3, 4, 5]));
                controller.close();
              },
            }),
            { headers: { 'content-length': '5' } },
          ),
        )) as typeof fetch;

      const progress: Array<{ downloaded: number; total?: number }> = [];
      const result = await downloadFile(
        'https://example.test/asset.bin',
        filePath,
        (value) => {
          progress.push({ downloaded: value.downloaded, total: value.total });
        },
      );
      assert.deepEqual(
        await Deno.readFile(filePath),
        new Uint8Array([1, 2, 3, 4, 5]),
      );
      assert.equal(result.size, 5);
      assert.deepEqual(progress, [
        { downloaded: 2, total: 5 },
        { downloaded: 5, total: 5 },
      ]);
    }),
});

Deno.test({
  name: 'downloadFile rejects unsuccessful HTTP responses',
  timeout: 5_000,
  fn: async () =>
    withTempFetch(async (root) => {
      const filePath = path.join(root, 'missing.bin');
      globalThis.fetch = (() =>
        Promise.resolve(
          new Response('missing', { status: 404 }),
        )) as typeof fetch;

      await assert.rejects(
        () => downloadFile('https://example.test/missing.bin', filePath),
        /download failed/,
      );
      assert.equal(await Deno.stat(filePath).catch(() => undefined), undefined);
    }),
});
