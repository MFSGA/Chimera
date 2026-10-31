import assert from 'node:assert/strict';
import * as path from 'node:path';

const script = path.join(import.meta.dirname!, 'prepare-nightly.ts');
const denoConfig = path.join(import.meta.dirname!, 'deno.jsonc');

async function git(cwd: string, args: string[]) {
  const result = await new Deno.Command('git', {
    args,
    cwd,
    stdout: 'piped',
    stderr: 'piped',
  }).output();
  assert.equal(result.code, 0, new TextDecoder().decode(result.stderr));
  return new TextDecoder().decode(result.stdout).trim();
}

async function writeJson(root: string, name: string, value: unknown) {
  const file = path.join(root, name);
  await Deno.mkdir(path.dirname(file), { recursive: true });
  await Deno.writeTextFile(file, JSON.stringify(value));
}

async function readJson(root: string, name: string) {
  return JSON.parse(await Deno.readTextFile(path.join(root, name)));
}

async function runPrepare(root: string, flags: string[]) {
  const result = await new Deno.Command(Deno.execPath(), {
    args: ['run', '--frozen', '--config', denoConfig, '-A', script, ...flags],
    cwd: root,
    stdout: 'piped',
    stderr: 'piped',
  }).output();
  assert.equal(
    result.code,
    0,
    `${flags.join(' ')}: ${new TextDecoder().decode(result.stderr)}`,
  );
  return {
    tauri: await readJson(root, 'backend/tauri/tauri.nightly.conf.json'),
    app: await readJson(root, 'frontend/chimera/package.json'),
    root: await readJson(root, 'package.json'),
  };
}

Deno.test(
  'Linux nightly uses an RPM-safe Tauri version while preserving updater SemVer',
  async () => {
    // The preparation script reads and writes only this isolated fake checkout.
    const root = await Deno.makeTempDir({ prefix: 'chimera-nightly-version-' });
    try {
      await writeJson(root, 'backend/tauri/tauri.conf.json', {
        version: '0.27.1',
        bundle: { targets: 'all', createUpdaterArtifacts: true },
      });
      await writeJson(root, 'backend/tauri/overrides/nightly.conf.json', {
        version: '0.15.3',
      });
      await writeJson(root, 'frontend/chimera/package.json', {
        version: '0.27.1',
      });
      await writeJson(root, 'package.json', { version: '0.27.1' });
      await git(root, ['init', '-q']);
      await git(root, [
        '-c',
        'user.name=Fixture',
        '-c',
        'user.email=fixture@example.invalid',
        'commit',
        '--allow-empty',
        '-qm',
        'fixture',
      ]);
      const hash = (await git(root, ['rev-parse', 'HEAD'])).slice(0, 7);
      const appVersion = `0.15.3-alpha+${hash}`;

      const linux = await runPrepare(root, ['--linux', '--disable-updater']);
      assert.equal(linux.tauri.version, `0.15.3+alpha.${hash}`);
      assert.match(linux.tauri.version, /^[a-zA-Z0-9._+%{}~^]+$/);
      assert.equal(linux.tauri.bundle.createUpdaterArtifacts, false);
      assert.equal(linux.app.version, appVersion);
      assert.equal(linux.root.version, appVersion);

      const linuxX64 = await runPrepare(root, ['--linux']);
      assert.equal(linuxX64.tauri.version, `0.15.3+alpha.${hash}`);
      assert.equal(linuxX64.tauri.bundle.createUpdaterArtifacts, true);
      assert.equal(linuxX64.app.version, appVersion);

      const mac = await runPrepare(root, []);
      assert.equal(mac.tauri.version, appVersion);
      assert.equal(mac.tauri.bundle.createUpdaterArtifacts, true);
      assert.equal(mac.app.version, appVersion);

      const nsis = await runPrepare(root, ['--nsis']);
      assert.equal(nsis.tauri.version, '0.15.3');
      assert.deepEqual(nsis.tauri.bundle.targets, ['nsis']);
      assert.equal(nsis.app.version, appVersion);
    } finally {
      await Deno.remove(root, { recursive: true });
    }
  },
);
