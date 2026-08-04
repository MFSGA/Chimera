import assert from 'node:assert/strict';
import test from 'node:test';
import {
  buildAgentIssueUrl,
  buildPrivacySafeAgentIssueContext,
  type AgentIssueSnapshot,
} from './issue-guidance';

test('issue URL pre-fills only supported privacy-safe fields', () => {
  const snapshot = {
    schema_version: 1,
    health: 'critical',
    findings: [{ code: 'weak_controller_secret' }],
    probe_failures: [{ code: 'telemetry_unavailable' }],
    token: 'token-canary',
    controller_secret: 'secret-canary',
    subscription_url: 'https://subscription-canary.example',
    connection_target: '10.0.0.1:9090',
    raw_logs: 'raw-log-canary',
  } as AgentIssueSnapshot & Record<string, unknown>;

  const url = new URL(
    buildAgentIssueUrl({
      actual: 'Agent repair did not resolve the issue.',
      envInfos: '> Chimera: 1.0.0',
      snapshot,
    }),
  );

  assert.equal(url.origin, 'https://github.com');
  assert.equal(url.pathname, '/MFSGA/Chimera/issues/new');
  assert.equal(url.searchParams.get('template'), 'bug_report.yaml');
  assert.equal(
    url.searchParams.get('actual'),
    'Agent repair did not resolve the issue.',
  );
  assert.equal(url.searchParams.get('env_infos'), '> Chimera: 1.0.0');

  const context = url.searchParams.get('more') ?? '';
  assert.match(context, /schema_version: 1/);
  assert.match(context, /health: critical/);
  assert.match(context, /finding_codes: weak_controller_secret/);
  assert.match(context, /probe_failure_codes: telemetry_unavailable/);

  for (const forbidden of [
    'token-canary',
    'secret-canary',
    'subscription-canary',
    '10.0.0.1:9090',
    'raw-log-canary',
  ]) {
    assert.equal(url.toString().includes(forbidden), false, forbidden);
  }
});

test('missing diagnostics produce an explicit safe fallback', () => {
  assert.equal(
    buildPrivacySafeAgentIssueContext(null),
    'Chimera Agent privacy-safe context\n- snapshot: unavailable',
  );
});
