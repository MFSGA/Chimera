import assert from 'node:assert/strict';
import * as path from 'node:path';
import { renameMacosUpdaterFiles } from './upload-macos-updater.ts';

Deno.test(
  'macOS updater rename preserves ref bundle naming using local files only',
  async () => {
    const dir = await Deno.makeTempDir({ prefix: 'chimera-macos-updater-' });
    try {
      const bundle = path.join(dir, 'release/bundle');
      await Deno.mkdir(bundle, { recursive: true });
      const originals = [
        'Chimera.app.tar.gz',
        'Chimera.app.tar.gz.sig',
        'Chimera_0.15.3.dmg',
        'unrelated.tar.gz',
      ];
      for (const name of originals) {
        await Deno.writeTextFile(path.join(bundle, name), name);
      }

      await renameMacosUpdaterFiles(dir, 'x86_64');

      for (const [original, renamed] of [
        ['Chimera.app.tar.gz', 'Chimera.x86_64.app.tar.gz'],
        ['Chimera.app.tar.gz.sig', 'Chimera.x86_64.app.tar.gz.sig'],
      ]) {
        assert.equal(
          await Deno.readTextFile(path.join(bundle, renamed)),
          original,
        );
        await assert.rejects(
          Deno.stat(path.join(bundle, original)),
          Deno.errors.NotFound,
        );
      }
      for (const name of ['Chimera_0.15.3.dmg', 'unrelated.tar.gz']) {
        assert.equal(await Deno.readTextFile(path.join(bundle, name)), name);
      }
    } finally {
      await Deno.remove(dir, { recursive: true });
    }
  },
);
