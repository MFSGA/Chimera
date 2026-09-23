import { execFileSync } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import https from 'node:https';
import path from 'node:path';
import {
  assertServiceReleaseCommit,
  assertStableServiceRelease,
  type ServiceReleaseMetadata,
} from './utils/service-release.ts';

const repo = 'MFSGA/Chimera_Service';
const root = process.cwd();

async function readPackageVersion(relativePath: string): Promise<string> {
  const manifest = await readFile(path.join(root, relativePath), 'utf8');
  const match = manifest.match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/ms);
  if (!match) {
    throw new Error(`failed to parse package version from ${relativePath}`);
  }
  return match[1];
}

const [serviceVersion, ipcVersion] = await Promise.all([
  readPackageVersion('backend/chimera-runtime/chimera_service/Cargo.toml'),
  readPackageVersion('backend/chimera-runtime/chimera_ipc/Cargo.toml'),
]);

if (serviceVersion !== ipcVersion) {
  throw new Error(
    `runtime version mismatch: chimera-service=${serviceVersion}, chimera-ipc=${ipcVersion}`,
  );
}

const tag = `v${serviceVersion}`;
const headers: Record<string, string> = {
  Accept: 'application/vnd.github+json',
  'User-Agent': 'chimera-release-preflight',
  'X-GitHub-Api-Version': '2022-11-28',
};
if (process.env.GITHUB_TOKEN) {
  headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
}

async function getGitHubJson<T>(apiPath: string): Promise<T> {
  const url = `https://api.github.com${apiPath}`;
  const response = await new Promise<{
    statusCode: number;
    statusMessage: string;
    body: string;
  }>((resolve, reject) => {
    const request = https.get(url, { headers }, (result) => {
      const chunks: Buffer[] = [];
      result.on('data', (chunk: Buffer) => chunks.push(chunk));
      result.on('end', () => {
        resolve({
          statusCode: result.statusCode ?? 0,
          statusMessage: result.statusMessage ?? '',
          body: Buffer.concat(chunks).toString('utf8'),
        });
      });
    });
    request.on('error', reject);
  });

  if (response.statusCode < 200 || response.statusCode >= 300) {
    throw new Error(
      `GitHub API request failed for ${apiPath}: HTTP ${response.statusCode} ${response.statusMessage}`,
    );
  }
  return JSON.parse(response.body) as T;
}

interface GitObjectRef {
  object: {
    type: 'commit' | 'tag';
    sha: string;
  };
}

interface GitAnnotatedTag {
  object: {
    type: 'commit' | 'tag';
    sha: string;
  };
}

async function resolveTagCommit(tagName: string): Promise<string> {
  let object = (
    await getGitHubJson<GitObjectRef>(
      `/repos/${repo}/git/ref/tags/${encodeURIComponent(tagName)}`,
    )
  ).object;

  for (let depth = 0; object.type === 'tag' && depth < 8; depth += 1) {
    object = (
      await getGitHubJson<GitAnnotatedTag>(
        `/repos/${repo}/git/tags/${object.sha}`,
      )
    ).object;
  }
  if (object.type !== 'commit') {
    throw new Error(`failed to resolve ${tagName} to a commit`);
  }
  return object.sha;
}

const release = await getGitHubJson<ServiceReleaseMetadata>(
  `/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`,
);
assertStableServiceRelease(release, tag);

const pinnedCommit = execFileSync(
  'git',
  ['-C', 'backend/chimera-runtime', 'rev-parse', 'HEAD'],
  {
    cwd: root,
    encoding: 'utf8',
    windowsHide: true,
  },
).trim();
const releaseCommit = await resolveTagCommit(tag);
assertServiceReleaseCommit(pinnedCommit, releaseCommit);

console.log(
  `stable Chimera Service dependency verified: ${tag} @ ${releaseCommit} with ${release.assets.length} assets`,
);
