import { mkdir, readFile, writeFile } from 'node:fs/promises';
import * as path from 'node:path';
import { parseArgs } from 'node:util';
import { Octokit } from 'octokit';
import { runCommand } from './src/shared/command.ts';
import { colorize, consola } from './src/shared/logger.ts';
import {
  assertUpdaterManifest,
  matchStableUpdaterAsset,
} from './updater-manifest.ts';

const GITHUB_PROXY = 'https://gh-proxy.com/';
const PRE_RELEASE_TAG = 'pre-release';
const UPDATE_TAG_NAME = 'updater';
const UPDATE_JSON_FILE = 'update-nightly.json';
const UPDATE_JSON_PROXY = 'update-nightly-proxy.json';
const UPDATE_FIXED_WEBVIEW_FILE = 'update-nightly-fixed-webview.json';
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-nightly-fixed-webview-proxy.json';

const { values: argv } = parseArgs({
  args: process.argv.slice(2),
  options: {
    'fixed-webview': { type: 'boolean', default: false },
    'cache-path': { type: 'string' },
  },
  strict: false,
});

type ReleaseAsset = {
  id: number;
  name: string;
  browser_download_url: string;
};

type UpdaterRelease = {
  id: number;
  assets: Array<{ id: number; name: string }>;
};

function getGithubUrl(url: string): string {
  return new URL(url.replace(/^https?:\/\//g, ''), GITHUB_PROXY).toString();
}

function getRepoContext() {
  const token = process.env.GITHUB_TOKEN;
  if (!token) throw new Error('GITHUB_TOKEN is required');

  const repoStr = process.env.GITHUB_REPOSITORY ?? '';
  const [owner, repo] = repoStr.split('/');
  if (!owner || !repo) throw new Error('GITHUB_REPOSITORY must be owner/repo');

  return { token, owner, repo };
}

async function readJsonFile<T>(filePath: string): Promise<T> {
  return JSON.parse(await readFile(filePath, 'utf8')) as T;
}

async function resolveNightlyVersion() {
  const nightlyPath = path.join(
    process.cwd(),
    'backend/tauri/overrides/nightly.conf.json',
  );
  const stablePath = path.join(process.cwd(), 'backend/tauri/tauri.conf.json');

  try {
    const nightly = await readJsonFile<{ version: string }>(nightlyPath);
    return nightly.version;
  } catch {
    const stable = await readJsonFile<{ version: string }>(stablePath);
    return stable.version;
  }
}

export function nightlyBuildHash(version: unknown): string | undefined {
  if (typeof version !== 'string') return undefined;
  // The updater must use the same fixed seven-byte SHA prefix signed into
  // prepare-nightly artifacts, not a variable-length abbreviated Git ID.
  const match =
    /^v?(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?\+([0-9a-f]{7,40})(?:\.[0-9A-Za-z.-]+)*$/.exec(
      version,
    );
  return match?.[1].slice(0, 7);
}

export async function resolveShortHash(
  assets: ReleaseAsset[],
  cwd = process.cwd(),
) {
  const latestContent = assets.find((asset) => asset.name === 'latest.json');
  if (latestContent) {
    try {
      const response = await fetch(latestContent.browser_download_url);
      if (!response.ok) throw new Error(`latest.json HTTP ${response.status}`);
      const latest = (await response.json()) as { version?: unknown };
      const hash = nightlyBuildHash(latest?.version);
      if (hash) return hash;
    } catch (err) {
      consola.error(err);
    }
  }

  const gitResult = await runCommand('git', {
    cwd,
    args: ['rev-parse', PRE_RELEASE_TAG],
  });
  if (!gitResult.success) {
    throw new Error(
      `failed to resolve pre-release tag: ${gitResult.stderr.toString('utf8')}`,
    );
  }
  const hash = gitResult.stdout.toString('utf8').trim().slice(0, 7);
  if (!/^[a-f0-9]{7}$/.test(hash))
    throw new Error('invalid pre-release commit hash');
  return hash;
}

async function getSignature(url: string) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
  });
  if (!response.ok) {
    throw new Error(`failed to fetch signature: ${response.status} ${url}`);
  }
  return response.text();
}

async function saveToCache(fileName: string, content: string) {
  const cachePath = argv['cache-path'];
  if (typeof cachePath !== 'string' || !cachePath) return;

  const filePath = path.join(cachePath, fileName);
  await mkdir(cachePath, { recursive: true });
  await writeFile(filePath, content);
  consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`);
}

function upperFirst(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function camelCase(s: string) {
  return s.replace(/-(\w)/g, (_, c: string) => c.toUpperCase());
}

function findUpdaterAssetPair(assets: ReleaseAsset[], assetName: string) {
  const asset = assets.find((candidate) => candidate.name === assetName);
  if (!asset) throw new Error(`missing updater asset ${assetName}`);
  const signature = assets.find(
    (candidate) => candidate.name === `${assetName}.sig`,
  );
  if (!signature) throw new Error(`missing updater signature ${assetName}.sig`);
  return signature;
}

export async function getOrCreateUpdaterRelease(
  github: Octokit,
  options: { owner: string; repo: string },
) {
  try {
    const { data } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: UPDATE_TAG_NAME,
    });
    return data as UpdaterRelease;
  } catch (err) {
    if (!(err instanceof Error && 'status' in err && err.status === 404)) {
      throw err;
    }
    consola.info('updater release tag missing, create it');
    const { data } = await github.rest.repos.createRelease({
      ...options,
      tag_name: UPDATE_TAG_NAME,
      name: upperFirst(camelCase(UPDATE_TAG_NAME)),
      body: 'files for programs to check for updates',
      prerelease: true,
    });
    return data as UpdaterRelease;
  }
}

async function deleteOldUpdaterAssets(
  github: Octokit,
  options: { owner: string; repo: string },
  updateRelease: UpdaterRelease,
) {
  for (const asset of updateRelease.assets) {
    if (
      argv['fixed-webview'] === true
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      });
    }

    if (
      argv['fixed-webview'] === true
        ? asset.name === UPDATE_FIXED_WEBVIEW_PROXY
        : asset.name === UPDATE_JSON_PROXY
    ) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err: unknown) => {
          consola.error(err);
        });
    }
  }
}

async function uploadUpdaterAsset(
  github: Octokit,
  options: { owner: string; repo: string },
  releaseId: number,
  name: string,
  content: string,
) {
  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: releaseId,
    name,
    data: content,
  });
  await saveToCache(name, content);
}

export async function createNightlyUpdaterPayloads(
  assets: ReleaseAsset[],
  nightlyVersion: string,
  shortHash: string,
  fixedWebview: boolean,
  signatureOf: (url: string) => Promise<string> = getSignature,
  now: () => string = () => new Date().toISOString(),
) {
  const platforms: Record<string, { signature: string; url: string }> = {};
  const updaterAssets = assets.filter(
    (asset) =>
      matchStableUpdaterAsset(asset.name, fixedWebview)?.kind === 'url',
  );
  for (const asset of updaterAssets) {
    const match = matchStableUpdaterAsset(asset.name, fixedWebview);
    if (match?.kind !== 'url') continue;
    const signature = findUpdaterAssetPair(assets, asset.name);
    const signatureValue = await signatureOf(signature.browser_download_url);
    for (const target of match.targets) {
      if (platforms[target])
        throw new Error(`multiple updater assets for ${target}`);
      platforms[target] = {
        signature: signatureValue,
        url: asset.browser_download_url,
      };
    }
  }
  const updateData = {
    version: `v${nightlyVersion}-alpha+${shortHash}`,
    notes: 'Nightly build. Full changes see commit history.',
    pub_date: now(),
    platforms,
  };
  const requiredTargets = fixedWebview
    ? ['windows-x86_64']
    : [
        'windows-x86_64',
        'windows-aarch64',
        'darwin',
        'darwin-aarch64',
        'linux',
      ];
  assertUpdaterManifest(updateData, requiredTargets);

  const proxyData = JSON.parse(JSON.stringify(updateData)) as typeof updateData;
  for (const value of Object.values(proxyData.platforms)) {
    value.url = getGithubUrl(value.url);
  }
  assertUpdaterManifest(proxyData, requiredTargets);
  return { updateData, proxyData };
}

export async function resolveUpdater() {
  consola.start('start to generate updater files');

  const { token, owner, repo } = getRepoContext();
  const github = new Octokit({ auth: token });
  const options = { owner, repo };

  consola.debug('resolve latest pre-release files...');
  const { data: latestPreRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: PRE_RELEASE_TAG,
  });

  const assets = latestPreRelease.assets as ReleaseAsset[];
  const shortHash = await resolveShortHash(assets);
  const nightlyVersion = await resolveNightlyVersion();
  consola.info(`latest pre-release short hash: ${shortHash}`);

  const { updateData, proxyData: updateDataNew } =
    await createNightlyUpdaterPayloads(
      assets,
      nightlyVersion,
      shortHash,
      argv['fixed-webview'] === true,
    );
  consola.info(updateData);

  consola.debug('update updater files...');
  const updateRelease = await getOrCreateUpdaterRelease(github, options);
  await deleteOldUpdaterAssets(github, options, updateRelease);

  const mainFileName =
    argv['fixed-webview'] === true
      ? UPDATE_FIXED_WEBVIEW_FILE
      : UPDATE_JSON_FILE;
  const proxyFileName =
    argv['fixed-webview'] === true
      ? UPDATE_FIXED_WEBVIEW_PROXY
      : UPDATE_JSON_PROXY;
  const mainContent = JSON.stringify(updateData, null, 2);
  const proxyContent = JSON.stringify(updateDataNew, null, 2);

  await uploadUpdaterAsset(
    github,
    options,
    updateRelease.id,
    mainFileName,
    mainContent,
  );
  await uploadUpdaterAsset(
    github,
    options,
    updateRelease.id,
    proxyFileName,
    proxyContent,
  );

  consola.success('updater files updated');
}

if (import.meta.main) {
  await resolveUpdater().catch((err) => {
    consola.fatal(err);
    process.exitCode = 1;
  });
}
