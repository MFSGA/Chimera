export interface SidecarReuseInput {
  force?: boolean;
  version?: string;
  targetExists: boolean;
}

export function canReuseExistingSidecar({
  force,
  version,
  targetExists,
}: SidecarReuseInput): boolean {
  return targetExists && !force && !version;
}
