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

// eslint-disable-next-line @typescript-eslint/no-namespace
export namespace Connection {
  export interface Item {
    id: string;
    metadata: Metadata;
    upload: number;
    download: number;
    start: string;
    chains: string[];
    rule: string;
    rulePayload: string;
  }

  export interface Metadata {
    network: string;
    type: string;
    host: string;
    sourceIP: string;
    sourcePort: string;
    destinationPort: string;
    destinationIP?: string;
    destinationIPASN?: string;
    process?: string;
    processPath?: string;
    dnsMode?: string;
    dscp?: number;
    inboundIP?: string;
    inboundName?: string;
    inboundPort?: string;
    inboundUser?: string;
    remoteDestination?: string;
    sniffHost?: string;
    specialProxy?: string;
    specialRules?: string;
  }

  export interface Response {
    downloadTotal: number;
    uploadTotal: number;
    memory?: number;
    connections?: Item[];
  }
}
