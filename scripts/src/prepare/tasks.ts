import { createBinaryResolvers } from './binaries.ts';
import {
  resolveResource,
  resolveSidecar,
  resolveWintun,
  type ResourceResolverOptions,
} from './resources.ts';
import type {
  DownloadProgress,
  ResolveInfo,
  VersionManifest,
} from './types.ts';

export interface Task {
  name: string;
  version?: string;
  func: (
    onProgress?: (progress: DownloadProgress) => void,
  ) => Promise<ResolveInfo>;
  retry: number;
  winOnly?: boolean;
}

interface CreatePrepareTasksOptions {
  force?: boolean;
  versionManifest: VersionManifest;
  binary: ReturnType<typeof createBinaryResolvers>;
  resourceContext: ResourceResolverOptions;
}

export function createPrepareTasks({
  force,
  versionManifest,
  binary,
  resourceContext,
}: CreatePrepareTasksOptions): Task[] {
  return [
    {
      name: 'clash',
      version: versionManifest.latest.clash_premium,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clash(), { force, onProgress }),
      retry: 5,
    },
    {
      name: 'mihomo',
      version: versionManifest.latest.mihomo,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.mihomo(), { force, onProgress }),
      retry: 5,
    },
    {
      name: 'mihomo-alpha',
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.mihomoAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'clash-rs',
      version: versionManifest.latest.clash_rs,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clashRs(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'clash-rs-alpha',
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.clashRsAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'chimera-client',
      version: versionManifest.latest.chimera_client,
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.chimeraClient(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'chimera-client-alpha',
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.chimeraClientAlpha(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'wintun',
      func: (onProgress) =>
        resolveWintun(resourceContext, force ?? false, onProgress),
      retry: 5,
      winOnly: true,
    },
    {
      name: 'chimera-service',
      func: (onProgress) =>
        resolveSidecar(resourceContext, binary.chimeraService(), {
          force,
          onProgress,
        }),
      retry: 5,
    },
    {
      name: 'mmdb',
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: 'Country.mmdb',
            downloadURL:
              'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/country.mmdb',
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: 'geoip',
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: 'geoip.dat',
            downloadURL:
              'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat',
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: 'geosite',
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: 'geosite.dat',
            downloadURL:
              'https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat',
          },
          { force, onProgress },
        ),
      retry: 5,
    },
    {
      name: 'enableLoopback',
      func: (onProgress) =>
        resolveResource(
          resourceContext,
          {
            file: 'enableLoopback.exe',
            downloadURL:
              'https://github.com/Kuingsmile/uwp-tool/releases/download/latest/enableLoopback.exe',
          },
          { force, onProgress },
        ),
      retry: 5,
      winOnly: true,
    },
  ];
}

export function selectPrepareTasks(tasks: Task[], platform: string): Task[] {
  return tasks.filter((task) => !task.winOnly || platform === 'win32');
}
