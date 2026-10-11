import type { AgentIntentResolution } from '@chimera/interface';

/**
 * Resolving an intent is not the same as completing a diagnosis.
 * Only disclose diagnosis success after the snapshot refresh succeeds.
 */
export async function resolveAgentIntentForDisplay(
  text: string,
  resolveIntent: (text: string) => Promise<AgentIntentResolution>,
  refreshDiagnosis: () => Promise<{ isSuccess: boolean }>,
): Promise<AgentIntentResolution | null> {
  const resolution = await resolveIntent(text);
  if (
    resolution.status !== 'resolved' ||
    resolution.intent.intent !== 'diagnose'
  ) {
    return resolution;
  }

  const refresh = await refreshDiagnosis();
  return refresh.isSuccess ? resolution : null;
}
