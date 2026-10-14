import { ensureDir, exists } from 'jsr:@std/fs';
import * as path from 'jsr:@std/path';
// @ts-types="npm:@types/adm-zip@0.5.8"
import AdmZip from 'npm:adm-zip@0.6.1';
import { verifyAndInstallSidecar } from '../../deno/utils/sidecar-integrity.ts';
import {
  downloadFile,
  extractTarGz,
  extractZip,
  gunzipFile,
  type DebugLog,
} from './download.ts';
import type {
  BinInfo,
  DownloadProgress,
  ResolveInfo,
  ResolveOptions,
} from './types.ts';

export interface ResourceResolverOptions {
  resourcesDir: string;
  sidecarDir: string;
  tempRoot: string;
  platform: string;
  arch: string;
  debugLog?: DebugLog;
}

export function isVersionManagedTarget(name: string): boolean {
  return ['chimera-service', 'chimera-client-alpha', 'mihomo-alpha'].includes(
    name,
  );
}

export function isSidecarCacheValid(
  name: string,
  targetExists: boolean,
  versionStamp: string | null,
  version?: string,
): boolean {
  if (!targetExists) return false;
  return !isVersionManagedTarget(name) || versionStamp === version;
}

export async function resolveResource(
  context: ResourceResolverOptions,
  binInfo: { file: string; downloadURL: string; version?: string },
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const { file, downloadURL, version } = binInfo;
  const targetPath = path.join(context.resourcesDir, file);
  const debugLog = context.debugLog ?? (() => {});

  if (!options?.force && (await exists(targetPath))) {
    return {
      file,
      version,
      size: (await Deno.stat(targetPath)).size,
      cached: true,
    };
  }

  await ensureDir(context.resourcesDir);
  const stagedPath = path.join(
    context.resourcesDir,
    `.${file}.${crypto.randomUUID()}.tmp`,
  );
  try {
    const { size, speed } = await downloadFile(
      downloadURL,
      stagedPath,
      (progress) => options?.onProgress?.({ ...progress, version }),
      debugLog,
    );
    await Deno.rename(stagedPath, targetPath);
    debugLog(`resolve ${file} finished`);
    return { file, version, size, speed, cached: false };
  } finally {
    try {
      await Deno.remove(stagedPath);
    } catch {
      // Ignore cleanup when the staged file was renamed or never created.
    }
  }
}

async function readVersionStamp(stampPath: string): Promise<string | null> {
  try {
    return (await Deno.readTextFile(stampPath)).trim();
  } catch {
    return null;
  }
}

export async function resolveSidecar(
  context: ResourceResolverOptions,
  binInfo: BinInfo | Promise<BinInfo>,
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const info = await binInfo;
  try {
    return await materializeSidecar(context, info, options);
  } catch (err) {
    if (!info.refresh) throw err;
    const debugLog = context.debugLog ?? (() => {});

    let refreshed: BinInfo;
    try {
      refreshed = await info.refresh();
    } catch (refreshErr) {
      debugLog(`failed to refresh ${info.name}: ${String(refreshErr)}`);
      throw err;
    }

    debugLog(`retrying ${info.name} with version ${refreshed.version}`);
    return await materializeSidecar(context, refreshed, options);
  }
}

async function materializeSidecar(
  context: ResourceResolverOptions,
  binInfo: BinInfo,
  options?: ResolveOptions,
): Promise<ResolveInfo> {
  const {
    name,
    version,
    targetFile,
    tmpFile,
    exeFile,
    downloadURL,
    checksumURL,
  } = binInfo;
  const sidecarPath = path.join(context.sidecarDir, targetFile);
  const versionStampPath = `${sidecarPath}.version`;
  const debugLog = context.debugLog ?? (() => {});

  await ensureDir(context.sidecarDir);

  // These cached targets are only trusted alongside a matching version stamp
  // written after successful materialization.
  const versionManagedTarget = isVersionManagedTarget(name);
  const cachedTargetIsValid = isSidecarCacheValid(
    name,
    await exists(sidecarPath),
    versionManagedTarget ? await readVersionStamp(versionStampPath) : null,
    version,
  );

  if (!options?.force && cachedTargetIsValid) {
    return {
      file: targetFile,
      version,
      size: (await Deno.stat(sidecarPath)).size,
      cached: true,
    };
  }

  const tempDir = path.join(context.tempRoot, name);
  const tempFile = path.join(tempDir, tmpFile);
  const tempExe = path.join(tempDir, exeFile);
  const checksumFile = `${tempFile}.sha256`;

  // Mutable/versioned assets must not reuse an archive downloaded for an older version.
  if (options?.force || versionManagedTarget) {
    try {
      await Deno.remove(tempDir, { recursive: true });
    } catch (error) {
      if (!(error instanceof Deno.errors.NotFound)) throw error;
    }
  }
  await ensureDir(tempDir);

  try {
    let size: number;
    let speed: number | undefined;
    if (!(await exists(tempFile))) {
      const result = await downloadFile(
        downloadURL,
        tempFile,
        (progress) => options?.onProgress?.({ ...progress, version }),
        debugLog,
      );
      size = result.size;
      speed = result.speed;
    } else {
      size = (await Deno.stat(tempFile)).size;
    }

    if (checksumURL) {
      await downloadFile(checksumURL, checksumFile, undefined, debugLog);
    }

    await verifyAndInstallSidecar({
      archivePath: tempFile,
      checksumPath: checksumURL ? checksumFile : undefined,
      targetPath: sidecarPath,
      versionStampPath:
        versionManagedTarget && version ? versionStampPath : undefined,
      version,
      stage: async (stagedPath) => {
        if (tmpFile.endsWith('.zip')) {
          const extractedExe = await extractZip(
            tempFile,
            tempDir,
            name,
            debugLog,
          );
          await Deno.copyFile(extractedExe, stagedPath);
        } else if (tmpFile.endsWith('.tar.gz')) {
          await extractTarGz(tempFile, tempDir);
          await Deno.copyFile(tempExe, stagedPath);
        } else if (tmpFile.endsWith('.gz')) {
          await gunzipFile(tempFile, stagedPath);
        } else {
          await Deno.copyFile(tempFile, stagedPath);
        }

        if (context.platform !== 'win32') await Deno.chmod(stagedPath, 0o755);
      },
    });

    if (checksumURL) {
      debugLog(`verified SHA-256 for ${name} ${version ?? ''}`);
    }

    debugLog(`resolve ${name} finished`);
    return { file: targetFile, version, size, speed, cached: false };
  } finally {
    try {
      await Deno.remove(tempDir, { recursive: true });
    } catch {
      // ignore
    }
  }
}

export async function resolveWintun(
  context: ResourceResolverOptions,
  force: boolean,
  onProgress?: (progress: DownloadProgress) => void,
): Promise<ResolveInfo> {
  if (context.platform !== 'win32') {
    return { file: 'wintun.dll', cached: true };
  }

  const wintunArchMap: Record<string, string> = {
    x64: 'amd64',
    ia32: 'x86',
    arm: 'arm',
    arm64: 'arm64',
  };
  const wintunArch = wintunArchMap[context.arch];
  if (!wintunArch) throw new Error(`unsupported arch ${context.arch}`);

  const url = 'https://www.wintun.net/builds/wintun-0.14.1.zip';
  const expectedHash =
    '07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51';
  const tempDir = path.join(context.tempRoot, 'wintun');
  const tempZip = path.join(tempDir, 'wintun.zip');
  const targetPath = path.join(context.resourcesDir, 'wintun.dll');
  const debugLog = context.debugLog ?? (() => {});

  if (!force && (await exists(targetPath))) {
    return {
      file: 'wintun.dll',
      size: (await Deno.stat(targetPath)).size,
      cached: true,
    };
  }

  try {
    // Never reuse a partially downloaded or invalid archive on the next retry.
    await Deno.remove(tempDir, { recursive: true });
  } catch (error) {
    if (!(error instanceof Deno.errors.NotFound)) throw error;
  }

  await ensureDir(tempDir);
  try {
    const { size, speed } = await downloadFile(
      url,
      tempZip,
      onProgress,
      debugLog,
    );

    const fileData = await Deno.readFile(tempZip);
    const hashBuffer = await crypto.subtle.digest('SHA-256', fileData);
    const hashHex = Array.from(new Uint8Array(hashBuffer))
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
    if (hashHex !== expectedHash) {
      throw new Error(`wintun hash not match ${hashHex}`);
    }

    const zip = new AdmZip(tempZip);
    zip.extractAllTo(tempDir, true);

    function findDlls(dir: string): string[] {
      const results: string[] = [];
      for (const entry of Deno.readDirSync(dir)) {
        const fullPath = path.join(dir, entry.name);
        if (entry.isDirectory) {
          results.push(...findDlls(fullPath));
        } else if (
          entry.name === 'wintun.dll' &&
          fullPath.includes(wintunArch)
        ) {
          results.push(fullPath);
        }
      }
      return results;
    }

    const dll = findDlls(tempDir)[0];
    if (!dll) throw new Error(`wintun not found for arch ${wintunArch}`);

    await ensureDir(path.dirname(targetPath));
    await verifyAndInstallSidecar({
      archivePath: tempZip,
      targetPath,
      stage: (stagedPath) => Deno.copyFile(dll, stagedPath),
    });
    debugLog('resolve wintun.dll finished');
    return { file: 'wintun.dll', size, speed, cached: false };
  } finally {
    try {
      await Deno.remove(tempDir, { recursive: true });
    } catch {
      // ignore cleanup errors after a successful install or failed attempt
    }
  }
}
