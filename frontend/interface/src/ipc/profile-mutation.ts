import type { RebuildOutcome } from './bindings';

export class ProfileChangeRolledBackError extends Error {
  constructor(reason: string) {
    super(`Profile change was rolled back: ${reason}`);
    this.name = 'ProfileChangeRolledBackError';
  }
}

export async function runProfileRebuildMutation(
  execute: () => Promise<RebuildOutcome | null | undefined>,
  invalidateProfiles: () => Promise<unknown>,
): Promise<RebuildOutcome> {
  const outcome = await execute();

  if (!outcome) {
    throw new Error('Profile change returned no rebuild outcome');
  }

  if (outcome.status === 'degraded') {
    throw new ProfileChangeRolledBackError(outcome.error);
  }

  await invalidateProfiles();
  return outcome;
}
