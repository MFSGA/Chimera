export type StartupStatusView = 'hidden' | 'loading' | 'failed';

export function getStartupStatusView(
  startupStatus: string | undefined,
  startupQueryFailed: boolean,
  dismissed: boolean,
): StartupStatusView {
  const failed = startupQueryFailed || startupStatus === 'failed';
  if (startupStatus === 'ready' || (failed && dismissed)) return 'hidden';
  return failed ? 'failed' : 'loading';
}
