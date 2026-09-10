export interface PrivacySafeSnapshot {
  privacy: {
    contains_raw_logs: boolean;
    contains_profile_names: boolean;
    contains_profile_urls: boolean;
    contains_connection_targets: boolean;
    contains_controller_secret: boolean;
  };
}

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
