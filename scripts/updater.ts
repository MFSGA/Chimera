import fs from 'fs/promises';
import path from 'path';
import { context, getOctokit } from '@actions/github';
import yargs from 'yargs';
import { hideBin } from 'yargs/helpers';
import { colorize, consola } from './utils/logger';

const UPDATE_TAG_NAME = 'updater';
const UPDATE_JSON_FILE = 'update.json';
const UPDATE_JSON_PROXY = 'update-proxy.json';
const UPDATE_FIXED_WEBVIEW_FILE = 'update-fixed-webview.json';
const UPDATE_FIXED_WEBVIEW_PROXY = 'update-fixed-webview-proxy.json';
const UPDATE_RELEASE_BODY = process.env.RELEASE_BODY || '';

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

/// generate update.json
/// upload to update tag's release asset
async function resolveUpdater() {
  if (process.env.GITHUB_TOKEN === undefined) {
    throw new Error('GITHUB_TOKEN is required');
  }

  const options = { owner: context.repo.owner, repo: context.repo.repo };
  const github = getOctokit(process.env.GITHUB_TOKEN);

  const { data: tags } = await github.rest.repos.listTags({
    ...options,
    per_page: 10,
    page: 1,
  });

  // get the latest publish tag
  const tag = tags.find((t) => t.name.startsWith('v'));
  if (!tag) throw new Error('could not found the latest tag');
  consola.debug(colorize`latest tag: {gray.bold ${tag.name}}`);

  const { data: latestRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: tag.name,
  });

  let updateLog: string | null = null;
  try {
    updateLog = 'updateLog'; //todo await resolveUpdateLog(tag.name)
  } catch (err) {
    consola.error(err);
  }

  const updateData = {
    name: tag.name,
    notes: UPDATE_RELEASE_BODY || updateLog || latestRelease.body,
    pub_date: new Date().toISOString(),
    platforms: {
      win64: { signature: '', url: '' }, // compatible with older formats
      'windows-x86_64': { signature: '', url: '' },
    },
  };

  const promises = latestRelease.assets.map(async (asset) => {
    const { name, browser_download_url: browserDownloadUrl } = asset;

    function isMatch(name: string, extension: string, arch: string) {
      return (
        name.endsWith(extension) &&
        name.includes(arch) &&
        (argv.fixedWebview
          ? name.includes('fixed-webview')
          : !name.includes('fixed-webview'))
      );
    }

    // win64 url
    // todo
    if (isMatch(name, '.exe', 'x64')) {
      updateData.platforms.win64.url = browserDownloadUrl;
      updateData.platforms['windows-x86_64'].url = browserDownloadUrl;
    }
    // win64 signature
    // todo
    if (isMatch(name, '.sig', 'x64')) {
      const sig = await getSignature(browserDownloadUrl);
      updateData.platforms.win64.signature = sig;
      updateData.platforms['windows-x86_64'].signature = sig;
    }
  });

  await Promise.allSettled(promises);
  consola.info(updateData);

  // maybe should test the signature as well
  // delete the null field
  Object.entries(updateData.platforms).forEach(([key, value]) => {
    if (!value.url) {
      consola.error(`failed to parse release for "${key}"`);
      delete updateData.platforms[key as keyof typeof updateData.platforms];
    }
  });

  // 生成一个代理github的更新文件
  // 使用 https://hub.fastgit.xyz/ 做github资源的加速
  const updateDataNew = JSON.parse(
    JSON.stringify(updateData),
  ) as typeof updateData;

  Object.entries(updateDataNew.platforms).forEach(([key, value]) => {
    if (value.url) {
      updateDataNew.platforms[key as keyof typeof updateData.platforms].url =
        value.url;
      // getGithubUrl(value.url);
    } else {
      consola.error(`updateDataNew.platforms.${key} is null`);
    }
  });

  // update the update.json
  const { data: updateRelease } = await github.rest.repos.getReleaseByTag({
    ...options,
    tag: UPDATE_TAG_NAME,
  });

  // delete the old assets
  for (const asset of updateRelease.assets) {
    if (
      argv.fixedWebview
        ? asset.name === UPDATE_FIXED_WEBVIEW_FILE
        : asset.name === UPDATE_JSON_FILE
    ) {
      await github.rest.repos.deleteReleaseAsset({
        ...options,
        asset_id: asset.id,
      });
    }

    if (
      argv.fixedWebview
        ? asset.name === UPDATE_FIXED_WEBVIEW_PROXY
        : asset.name === UPDATE_JSON_PROXY
    ) {
      await github.rest.repos
        .deleteReleaseAsset({ ...options, asset_id: asset.id })
        .catch((err) => {
          consola.error(err);
        }); // do not break the pipeline
    }
  }

  // upload new assets
  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE,
    data: JSON.stringify(updateData, null, 2),
  });

  // cache the files if cache path is provided
  await saveToCache(
    argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_FILE : UPDATE_JSON_FILE,
    JSON.stringify(updateData, null, 2),
  );

  await github.rest.repos.uploadReleaseAsset({
    ...options,
    release_id: updateRelease.id,
    name: argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_PROXY : UPDATE_JSON_PROXY,
    data: JSON.stringify(updateDataNew, null, 2),
  });

  // cache the proxy file if cache path is provided
  await saveToCache(
    argv.fixedWebview ? UPDATE_FIXED_WEBVIEW_PROXY : UPDATE_JSON_PROXY,
    JSON.stringify(updateDataNew, null, 2),
  );
}

async function saveToCache(fileName: string, content: string) {
  if (!argv.cachePath) return;

  try {
    await fs.mkdir(argv.cachePath, { recursive: true });
    const filePath = path.join(argv.cachePath, fileName);
    await fs.writeFile(filePath, content, 'utf-8');
    consola.success(colorize`cached file saved to: {gray.bold ${filePath}}`);
  } catch (err) {
    consola.error(`Failed to save cache file: ${err}`);
  }
}

// get the signature file content
async function getSignature(url: string) {
  const response = await fetch(url, {
    method: 'GET',
    headers: { 'Content-Type': 'application/octet-stream' },
    dispatcher: undefined,
  });

  return response.text();
}

resolveUpdater().catch((err) => {
  consola.error(err);
});
