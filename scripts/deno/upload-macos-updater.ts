// Native path handling avoids a release-blocking JSR fetch at this step.
import * as path from 'node:path';
import { globby } from 'npm:globby';
import { consola } from './utils/logger.ts';

const WORKSPACE_ROOT = path.join(import.meta.dirname!, '../..');
const TARGET_ARCH = Deno.env.get('TARGET_ARCH') || Deno.build.arch;
const BACKEND_BUILD_DIR = path.join(WORKSPACE_ROOT, 'backend/target');

/** Ref updater naming preserved, with an injectable build directory for tests. */
export async function renameMacosUpdaterFiles(
  backendBuildDir: string,
  targetArch: string,
) {
  const files = await globby(['**/*.tar.gz', '**/*.sig', '**/*.dmg'], {
    cwd: backendBuildDir,
  });

  for (let file of files) {
    file = path.join(backendBuildDir, file);
    const p = path.parse(file);
    consola.info(`Found file: ${p.base}`);
    if (p.base.includes('.app')) {
      const components = p.base.split('.');
      const newName =
        components[0] + `.${targetArch}.${components.slice(1).join('.')}`;
      const newPath = path.join(p.dir, newName);
      consola.info(`Renaming ${file} to ${newPath}`);
      await Deno.rename(file, newPath);
    }
  }
}

if (import.meta.main) {
  consola.info(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`);
  await renameMacosUpdaterFiles(BACKEND_BUILD_DIR, TARGET_ARCH);
  consola.success('Files renamed successfully');
}
