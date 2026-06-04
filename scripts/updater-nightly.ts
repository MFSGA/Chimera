import fs from 'fs/promises';
import path from 'path';
import { context, getOctokit } from '@actions/github';
import yargs from 'yargs';
import { hideBin } from 'yargs/helpers';
import { colorize, consola } from './utils/logger';

const PRE_RELEASE_TAG = 'pre-release';
const UPDATE_TAG_NAME = 'updater';
const UPDATE_JSON_FILE = 'update-nightly.json';
const UPDATE_JSON_PROXY = 'update-nightly-proxy.json';
const UPDATE_FIXED_WEBVIEW_FILE = 'update-nightly-fixed-webview.json';
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-nightly-fixed-webview-proxy.json';

const argv = yargs(hideBin(process.argv))
  .option('fixed-webview', {
    type: 'boolean',
    default: false,
  })
  .option('cache-path', {
    type: 'string',
    requiresArg: false,
  })
  .help()
  .parseSync();

type ReleaseAsset = {
  id: number;
  name: string;
  browser_download_url: string;
};

function shouldUseAsset(name: string) {
  return argv.fixedWebview
    ? name.includes('fixed-webview')
    : !name.includes('fixed-webview');
}

function findAsset(assets: ReleaseAsset[], suffix: string) {
  return assets.find(
    (asset) =>
      asset.name.endsWith(suffix) &&
      asset.name.includes('x64') &&
      shouldUseAsset(asset.name),
  );
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
  if (!argv.cachePath) return;

  const filePath = path.join(argv.cachePath, fileName);
  await fs.mkdir(argv.cachePath, { recursive: true });
  await fs.writeFile(filePath, content, 'utf-8');
  consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`);
}

async function getOrCreateUpdaterRelease(
  github: ReturnType<typeof getOctokit>,
  options: { owner: string; repo: string },
) {
  try {
    const { data } = await github.rest.repos.getReleaseByTag({
      ...options,
      tag: UPDATE_TAG_NAME,
    });
    return data;
  } catch {
    const { data } = await github.rest.repos.createRelease({
      ...options,
      tag_name: UPDATE_TAG_NAME,
      name: 'Updater',
      body: 'Files for programs to check for updates.',
      prerelease: true,
    });
    return data;
  }
}

async function replaceReleaseAsset(
  github: ReturnType<typeof getOctokit>,
  options: { owner: string; repo: string },
  release: { id: number; assets: Array<{ id: number; name: string }> },
  name: string,
  content: string,
) {
  const oldAsset = release.assets.find((asset) => asset.name === name);
  if (oldAsset) {
    await github.rest.repos.deleteReleaseAsset({
      ...options,
      asset_id: oldAsset.id,
    });
  }

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: release.id,
    name,
    data: content,
  });
}

async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required');
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo };
  const github = getOctokit(process.env.GITHUB_TOKEN);

  const { data: preRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: PRE_RELEASE_TAG,
  });

  const assets = preRelease.assets as ReleaseAsset[];
  const packageAsset =
    findAsset(assets, '.nsis.zip') ?? findAsset(assets, '.exe');
  const signatureAsset =
    findAsset(assets, '.nsis.zip.sig') ?? findAsset(assets, '.sig');

  if (!packageAsset || !signatureAsset) {
    throw new Error('failed to find Windows x64 nightly updater assets');
  }

  const signature = await getSignature(signatureAsset.browser_download_url);
  const updateData = {
    name: PRE_RELEASE_TAG,
    notes: preRelease.body || 'Nightly build.',
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

  const mainFileName = argv.fixedWebview
    ? UPDATE_FIXED_WEBVIEW_FILE
    : UPDATE_JSON_FILE;
  const proxyFileName = argv.fixedWebview
    ? UPDATE_FIXED_WEBVIEW_PROXY
    : UPDATE_JSON_PROXY;
  const mainContent = JSON.stringify(updateData, null, 2);
  const proxyContent = JSON.stringify(updateData, null, 2);
  const updateRelease = await getOrCreateUpdaterRelease(github, options);

  await replaceReleaseAsset(
    github,
    options,
    updateRelease,
    mainFileName,
    mainContent,
  );
  await saveToCache(mainFileName, mainContent);

  await replaceReleaseAsset(
    github,
    options,
    updateRelease,
    proxyFileName,
    proxyContent,
  );
  await saveToCache(proxyFileName, proxyContent);
}

resolveUpdater().catch((err) => {
  consola.fatal(err);
  process.exit(1);
});
