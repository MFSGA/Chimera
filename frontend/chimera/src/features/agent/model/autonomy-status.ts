import type { AgentAutonomyPolicyStatus } from '@chimera/interface';

export type AutonomyStatusPresentation =
  'active' | 'expired' | 'revoked' | 'session_mismatch' | 'inactive';

/** Collapse backend policy states into stable user-facing categories. */
export const presentAutonomyStatus = (
  status: AgentAutonomyPolicyStatus | undefined,
): AutonomyStatusPresentation => {
  switch (status) {
    case 'active':
      return 'active';
    case 'expired':
      return 'expired';
    case 'revoked':
      return 'revoked';
    case 'session_mismatch':
      return 'session_mismatch';
    default:
      return 'inactive';
  }
};
