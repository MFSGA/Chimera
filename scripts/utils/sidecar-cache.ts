export interface SidecarReuseInput {
  force?: boolean;
  version?: string;
  cachedVersion?: string | null;
  targetExists: boolean;
}

export function canReuseExistingSidecar({
  force,
  version,
  cachedVersion,
  targetExists,
}: SidecarReuseInput): boolean {
  if (!targetExists || force) return false;
  return !version || cachedVersion === version;
}
