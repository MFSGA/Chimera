import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const entry = path.join(import.meta.dirname!, 'prepare-nightly.ts');

async function writeJson(root: string, target: string, value: unknown) {
  const file = path.join(root, target);
  await mkdir(path.dirname(file), { recursive: true });
  await writeFile(file, JSON.stringify(value));
}

async function readJson(root: string, target: string) {
  return JSON.parse(await readFile(path.join(root, target), 'utf8'));
}

async function fixture(run: (root: string, hash: string) => Promise<void>) {
  const root = await mkdtemp(path.join(tmpdir(), 'chimera-nightly-node-'));
  try {
    await writeJson(root, 'backend/tauri/tauri.conf.json', {
      version: '0.27.1',
      bundle: {
        targets: ['nsis', 'msi'],
        createUpdaterArtifacts: true,
        windows: {
          webviewInstallMode: { silent: true, type: 'downloadBootstrapper' },
        },
      },
      plugins: {
        updater: {
          endpoints: [
            'https://example.test/update.json',
            'https://example.test/update-proxy.json',
          ],
        },
      },
    });
    await writeJson(root, 'backend/tauri/overrides/nightly.conf.json', {
      version: '0.15.3',
    });
    await writeJson(root, 'backend/tauri/overrides/fixed-webview2.conf.json', {
      bundle: {
        windows: {
          webviewInstallMode: { type: 'fixedRuntime', path: 'placeholder' },
        },
      },
      plugins: {
        updater: {
          endpoints: ['https://example.test/update-fixed-webview.json'],
        },
      },
    });
    await writeJson(root, 'frontend/chimera/package.json', {
      version: '0.27.1',
    });
    await writeJson(root, 'package.json', { version: '0.27.1' });

    await exec('git', ['init', '-q'], { cwd: root });
    await exec(
      'git',
      [
        '-c',
        'user.name=Fixture',
        '-c',
        'user.email=fixture@example.invalid',
        'commit',
        '--allow-empty',
        '-qm',
        'fixture',
      ],
      { cwd: root },
    );
    const { stdout } = await exec('git', ['rev-parse', 'HEAD'], { cwd: root });
    await run(root, stdout.trim().slice(0, 7));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

async function prepare(root: string, args: string[]) {
  await exec(process.execPath, [entry, ...args], { cwd: root });
  return {
    tauri: await readJson(root, 'backend/tauri/tauri.nightly.conf.json'),
    app: await readJson(root, 'frontend/chimera/package.json'),
    root: await readJson(root, 'package.json'),
  };
}

test('Linux nightly uses RPM-safe version without breaking updater SemVer', async () => {
  await fixture(async (root, hash) => {
    const result = await prepare(root, ['--linux', '--disable-updater']);
    assert.equal(result.tauri.version, `0.15.3+alpha.${hash}`);
    assert.match(result.tauri.version, /^[a-zA-Z0-9._+%{}~^]+$/);
    assert.equal(result.tauri.bundle.createUpdaterArtifacts, false);
    assert.equal(result.app.version, `0.15.3-alpha+${hash}`);
    assert.equal(result.root.version, result.app.version);

    const enabled = await prepare(root, ['--linux']);
    assert.equal(enabled.tauri.bundle.createUpdaterArtifacts, true);
    assert.equal(enabled.tauri.version, `0.15.3+alpha.${hash}`);

    const mac = await prepare(root, []);
    assert.equal(mac.tauri.version, `0.15.3-alpha+${hash}`);
    assert.deepEqual(mac.tauri.bundle.targets, ['nsis', 'msi']);

    const nsis = await prepare(root, ['--nsis']);
    assert.equal(nsis.tauri.version, '0.15.3');
    assert.deepEqual(nsis.tauri.bundle.targets, ['nsis']);
    assert.equal(nsis.app.version, `0.15.3-alpha+${hash}`);
    const msi = await prepare(root, ['--msi']);
    assert.equal(msi.tauri.version, '0.15.3');
  });
});

test('fixed WebView nightly preserves Chimera endpoints and uses local extracted runtime', async () => {
  await fixture(async (root) => {
    await mkdir(
      path.join(
        root,
        'backend/tauri/Microsoft.WebView2.FixedVersionRuntime.127.0',
      ),
      { recursive: true },
    );
    const result = await prepare(root, ['--nsis', '--fixed-webview']);
    assert.equal(
      result.tauri.bundle.windows.webviewInstallMode.path,
      './Microsoft.WebView2.FixedVersionRuntime.127.0',
    );
    assert.equal(
      result.tauri.bundle.windows.webviewInstallMode.silent,
      undefined,
    );
    assert.equal(
      result.tauri.plugins.updater.endpoints[0],
      'https://example.test/update-nightly-fixed-webview.json',
    );
    assert.equal(
      result.tauri.plugins.updater.endpoints[1],
      'https://example.test/update-nightly-proxy.json',
    );
  });
});

test('missing fixed WebView runtime aborts without altering previous files', async () => {
  await fixture(async (root) => {
    const rootFile = path.join(root, 'package.json');
    const original = await readFile(rootFile, 'utf8');
    await assert.rejects(
      exec(process.execPath, [entry, '--fixed-webview'], { cwd: root }),
      /WebView2 runtime not found/,
    );
    assert.equal(await readFile(rootFile, 'utf8'), original);
    await assert.rejects(
      readFile(path.join(root, 'backend/tauri/tauri.nightly.conf.json')),
      { code: 'ENOENT' },
    );
  });
});

test('Git revision lookup failure does not write nightly configuration', async () => {
  const root = await mkdtemp(
    path.join(tmpdir(), 'chimera-nightly-invalid-git-'),
  );
  try {
    await writeJson(root, 'backend/tauri/tauri.conf.json', {
      version: '0.28.2',
      bundle: {},
    });
    await writeJson(root, 'backend/tauri/overrides/nightly.conf.json', {});
    await writeJson(root, 'frontend/chimera/package.json', {
      version: '0.28.2',
    });
    await writeJson(root, 'package.json', { version: '0.28.2' });
    await assert.rejects(
      exec(process.execPath, [entry], { cwd: root }),
      /failed to resolve nightly Git commit/,
    );
    assert.equal((await readJson(root, 'package.json')).version, '0.28.2');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
