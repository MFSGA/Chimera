import type { UpdaterSummary } from '../ipc/bindings';

export type InspectUpdater = UpdaterSummary;

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
