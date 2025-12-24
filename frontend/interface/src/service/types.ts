export interface InspectUpdater {
  id: number;
  state:
    | 'idle'
    | 'downloading'
    | 'decompressing'
    | 'replacing'
    | 'restarting'
    | 'done'
    | { failed: string };
  downloader: {
    state:
      | 'idle'
      | 'downloading'
      | 'waiting_for_merge'
      | 'merging'
      | { failed: string }
      | 'finished';
    downloaded: number;
    total: number;
    speed: number;
    chunks: Array<{
      state: 'idle' | 'downloading' | 'finished';
      start: number;
      end: number;
      downloaded: number;
      speed: number;
    }>;
    now: number;
  };
}
