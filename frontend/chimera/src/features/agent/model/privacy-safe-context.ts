export interface PrivacySafeSnapshot {
  privacy: {
    contains_raw_logs: boolean;
    contains_profile_names: boolean;
    contains_profile_urls: boolean;
    contains_connection_targets: boolean;
    contains_controller_secret: boolean;
  };
}

export interface PrivacySafeIssueSource extends PrivacySafeSnapshot {
  schema_version: number;
  health: string;
  findings: Array<{ code: string }>;
  probe_failures: Array<{ code: string }>;
}

export type PrivacySafeIssueSnapshot = Pick<
  PrivacySafeIssueSource,
  'schema_version' | 'health' | 'findings' | 'probe_failures'
>;

/** Fail closed before rendering or copying the complete diagnostic snapshot. */
export const isPrivacySafeSnapshot = (snapshot: PrivacySafeSnapshot): boolean =>
  snapshot.privacy.contains_raw_logs === false &&
  snapshot.privacy.contains_profile_names === false &&
  snapshot.privacy.contains_profile_urls === false &&
  snapshot.privacy.contains_connection_targets === false &&
  snapshot.privacy.contains_controller_secret === false;

/** Serialize only snapshots whose explicit privacy assertions are all negative. */
export const serializePrivacySafeSnapshot = (
  snapshot: PrivacySafeSnapshot,
): string | null =>
  isPrivacySafeSnapshot(snapshot) ? JSON.stringify(snapshot, null, 2) : null;

/** Project a narrow issue-report payload only after the complete snapshot passes the privacy gate. */
export const projectPrivacySafeIssueSnapshot = (
  snapshot: PrivacySafeIssueSource | null | undefined,
): PrivacySafeIssueSnapshot | null => {
  if (!snapshot || !isPrivacySafeSnapshot(snapshot)) return null;
  return {
    schema_version: snapshot.schema_version,
    health: snapshot.health,
    findings: snapshot.findings,
    probe_failures: snapshot.probe_failures,
  };
};
