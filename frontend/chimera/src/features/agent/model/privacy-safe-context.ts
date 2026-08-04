import type { AgentNetworkSnapshot } from '@chimera/interface';

/** Fail closed before rendering or copying the complete diagnostic snapshot. */
export const isPrivacySafeSnapshot = (
  snapshot: AgentNetworkSnapshot,
): boolean =>
  snapshot.privacy.contains_raw_logs === false &&
  snapshot.privacy.contains_profile_names === false &&
  snapshot.privacy.contains_profile_urls === false &&
  snapshot.privacy.contains_connection_targets === false &&
  snapshot.privacy.contains_controller_secret === false;

export type PrivacySafeAgentIssueSnapshot = Pick<
  AgentNetworkSnapshot,
  'schema_version' | 'health' | 'findings' | 'probe_failures'
>;

/** Serialize only snapshots whose explicit privacy assertions are all negative. */
export const serializePrivacySafeSnapshot = (
  snapshot: AgentNetworkSnapshot,
): string | null =>
  isPrivacySafeSnapshot(snapshot) ? JSON.stringify(snapshot, null, 2) : null;

/** Project the narrow Issue payload only after the complete snapshot passes the privacy gate. */
export const projectPrivacySafeIssueSnapshot = (
  snapshot: AgentNetworkSnapshot | null | undefined,
): PrivacySafeAgentIssueSnapshot | null => {
  if (!snapshot || !isPrivacySafeSnapshot(snapshot)) return null;
  return {
    schema_version: snapshot.schema_version,
    health: snapshot.health,
    findings: snapshot.findings,
    probe_failures: snapshot.probe_failures,
  };
};
