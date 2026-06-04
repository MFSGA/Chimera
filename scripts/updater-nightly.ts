/// <reference lib="deno.ns" />
/// <reference lib="dom" />

import { parseArgs } from 'jsr:@std/cli@1/parse-args';
import * as path from 'jsr:@std/path';
import { Octokit } from 'npm:octokit';
import { colorize, consola } from './deno/utils/logger.ts';

const GITHUB_PROXY = 'https://gh-proxy.com/';
const PRE_RELEASE_TAG = 'pre-release';
const UPDATE_TAG_NAME = 'updater';
const UPDATE_JSON_FILE = 'update-nightly.json';
const UPDATE_JSON_PROXY = 'update-nightly-proxy.json';
const UPDATE_FIXED_WEBVIEW_FILE = 'update-nightly-fixed-webview.json';
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-nightly-fixed-webview-proxy.json';

const argv = parseArgs(Deno.args, {
  boolean: ['fixed-webview'],
  string: ['cache-path'],
  default: { 'fixed-webview': false },
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
  const token = Deno.env.get('GITHUB_TOKEN');
  if (!token) throw new Error('GITHUB_TOKEN is required');

  const repoStr = Deno.env.get('GITHUB_REPOSITORY') ?? '';
  const [owner, repo] = repoStr.split('/');
  if (!owner || !repo) throw new Error('GITHUB_REPOSITORY must be owner/repo');

  return { token, owner, repo };
}

async function readJsonFile<T>(filePath: string): Promise<T> {
  return JSON.parse(await Deno.readTextFile(filePath)) as T;
}

async function resolveNightlyVersion() {
  const nightlyPath = path.join(
    Deno.cwd(),
    'backend/tauri/overrides/nightly.conf.json',
  );
  const stablePath = path.join(Deno.cwd(), 'backend/tauri/tauri.conf.json');

  try {
    const nightly = await readJsonFile<{ version: string }>(nightlyPath);
    return nightly.version;
  } catch {
    const stable = await readJsonFile<{ version: string }>(stablePath);
    return stable.version;
  }
}

function getBuildMetadata(version: string) {
  const build = version.split('+')[1];
  return build?.split('.')[0] ?? '';
}

async function resolveShortHash(assets: ReleaseAsset[]) {
  const latestContent = assets.find((asset) => asset.name === 'latest.json');
  if (latestContent) {
    try {
      const latest = (await fetch(latestContent.browser_download_url).then(
        (res) => res.json(),
      )) as { version?: string };
      const shortHash = latest.version ? getBuildMetadata(latest.version) : '';
      if (shortHash) return shortHash;
    } catch (err) {
      consola.error(err);
    }
  }

  const gitResult = await new Deno.Command('git', {
    args: ['rev-parse', '--short', PRE_RELEASE_TAG],
    stdout: 'piped',
  }).output();
  return new TextDecoder().decode(gitResult.stdout).trim().slice(0, 7);
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
  if (!cachePath) return;

  const filePath = path.join(cachePath, fileName);
  await Deno.mkdir(cachePath, { recursive: true });
  await Deno.writeTextFile(filePath, content);
  consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`);
}

function upperFirst(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}

function camelCase(s: string) {
  return s.replace(/-(\w)/g, (_, c: string) => c.toUpperCase());
}

function isMatch(name: string, extension: string, arch: string) {
  return (
    name.endsWith(extension) &&
    name.includes(arch) &&
    (argv['fixed-webview']
      ? name.includes('fixed-webview')
      : !name.includes('fixed-webview'))
  );
}

function findAsset(
  assets: ReleaseAsset[],
  candidates: Array<{ extension: string; arch: string }>,
) {
  for (const candidate of candidates) {
    const asset = assets.find((item) =>
      isMatch(item.name, candidate.extension, candidate.arch),
    );
    if (asset) return asset;
  }
}

async function getOrCreateUpdaterRelease(
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
    consola.error(err);
    consola.error('failed to get release by tag, create one');
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
      argv['fixed-webview']
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      });
    }

    if (
      argv['fixed-webview']
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

async function resolveUpdater() {
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

  const packageAsset = findAsset(assets, [
    { extension: '.nsis.zip', arch: 'x64' },
    { extension: '.exe', arch: 'x64' },
  ]);
  const signatureAsset = findAsset(assets, [
    { extension: '.nsis.zip.sig', arch: 'x64' },
    { extension: '.sig', arch: 'x64' },
  ]);

  if (!packageAsset || !signatureAsset) {
    throw new Error('failed to parse release for "windows-x86_64"');
  }

  const signature = await getSignature(signatureAsset.browser_download_url);
  const updateData = {
    name: `v${nightlyVersion}-alpha+${shortHash}`,
    notes: 'Nightly build. Full changes see commit history.',
    pub_date: new Date().toISOString(),
    platforms: {
      win64: {
        signature,
        url: packageAsset.browser_download_url,
      },
      'windows-x86_64': {
        signature,
        url: packageAsset.browser_download_url,
      },
    },
  };
  consola.info(updateData);

  const updateDataNew = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData;
  Object.entries(updateDataNew.platforms).forEach(([key, value]) => {
    if (value.url) {
      updateDataNew.platforms[key as keyof typeof updateData.platforms].url =
        getGithubUrl(value.url);
    } else {
      consola.error(`updateDataNew.platforms.${key} is null`);
    }
  });

  consola.debug('update updater files...');
  const updateRelease = await getOrCreateUpdaterRelease(github, options);
  await deleteOldUpdaterAssets(github, options, updateRelease);

  const mainFileName = argv['fixed-webview']
    ? UPDATE_FIXED_WEBVIEW_FILE
    : UPDATE_JSON_FILE;
  const proxyFileName = argv['fixed-webview']
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

resolveUpdater().catch((err) => {
  consola.fatal(err);
  Deno.exit(1);
});
