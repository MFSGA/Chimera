import { readFile } from 'node:fs/promises';
import https from 'node:https';
import path from 'node:path';
import {
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

const releaseUrl = `https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(tag)}`;
const response = await new Promise<{
  statusCode: number;
  statusMessage: string;
  body: string;
}>((resolve, reject) => {
  const request = https.get(releaseUrl, { headers }, (result) => {
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
    `failed to resolve stable service release ${tag}: HTTP ${response.statusCode} ${response.statusMessage}`,
  );
}

const release = JSON.parse(response.body) as ServiceReleaseMetadata;
assertStableServiceRelease(release, tag);
console.log(
  `stable Chimera Service dependency verified: ${tag} with ${release.assets.length} assets`,
);
