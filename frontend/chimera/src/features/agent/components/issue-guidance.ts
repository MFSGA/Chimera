export type AgentIssueSnapshot = {
  schema_version: number;
  health: string;
  findings: Array<{ code: string }>;
  probe_failures: Array<{ code: string }>;
};

type AgentIssueUrlOptions = {
  actual: string;
  envInfos: string;
  snapshot: AgentIssueSnapshot | null | undefined;
};

const AGENT_ISSUE_FORM_URL =
  'https://github.com/MFSGA/Chimera/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml';

const listCodes = (entries: Array<{ code: string }>) =>
  entries.length > 0 ? entries.map(({ code }) => code).join(', ') : 'none';

export const buildPrivacySafeAgentIssueContext = (
  snapshot: AgentIssueSnapshot | null | undefined,
) => {
  if (!snapshot) {
    return [
      'Chimera Agent privacy-safe context',
      '- snapshot: unavailable',
    ].join('\n');
  }

  return [
    'Chimera Agent privacy-safe context',
    `- schema_version: ${snapshot.schema_version}`,
    `- health: ${snapshot.health}`,
    `- finding_codes: ${listCodes(snapshot.findings)}`,
    `- probe_failure_codes: ${listCodes(snapshot.probe_failures)}`,
  ].join('\n');
};

export const buildAgentIssueUrl = ({
  actual,
  envInfos,
  snapshot,
}: AgentIssueUrlOptions) => {
  const url = new URL(AGENT_ISSUE_FORM_URL);
  url.searchParams.set('actual', actual);
  url.searchParams.set('more', buildPrivacySafeAgentIssueContext(snapshot));
  url.searchParams.set('env_infos', envInfos);
  return url.toString();
};
