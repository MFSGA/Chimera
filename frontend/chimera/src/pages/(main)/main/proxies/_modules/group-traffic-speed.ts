export type GroupTrafficSpeed = { download: number; upload: number };

type ConnectionRate = {
  chains: readonly string[];
  downloadSpeed: number;
  uploadSpeed: number;
};

// In ref this reads ClashConnectionsSummary.memberRates; Chimera's existing
// shared detail stream supplies one speed per connection instead. A group
// can be present in several connections and in nested chains, so sum all
// matching connections in the latest frame without double-counting one.
export function latestGroupTrafficSpeed(
  snapshots: readonly { connections: readonly ConnectionRate[] }[],
  groupName: string,
): GroupTrafficSpeed {
  const current = snapshots.at(-1);
  if (!current) return { download: 0, upload: 0 };

  let download = 0;
  let upload = 0;
  for (const connection of current.connections) {
    if (!connection.chains.includes(groupName)) continue;
    download += connection.downloadSpeed;
    upload += connection.uploadSpeed;
  }
  return { download, upload };
}
