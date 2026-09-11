import assert from 'node:assert/strict';
import test from 'node:test';
import { buildAgentIssueUrl } from '../frontend/chimera/src/features/agent/components/issue-guidance.ts';
import { projectPrivacySafeIssueSnapshot } from '../frontend/chimera/src/features/agent/model/privacy-safe-context.ts';

const safePrivacy = {
  contains_raw_logs: false,
  contains_profile_names: false,
  contains_profile_urls: false,
  contains_connection_targets: false,
  contains_controller_secret: false,
};

const snapshot = {
  schema_version: 1,
  health: 'critical',
  findings: [{ code: 'weak_controller_secret' }],
  probe_failures: [{ code: 'telemetry_unavailable' }],
  privacy: safePrivacy,
  token: 'token-canary',
  controller_secret: 'secret-canary',
  subscription_url: 'https://subscription-canary.example',
  connection_target: '10.0.0.1:9090',
  raw_logs: 'raw-log-canary',
};

test('agent issue URL includes only projected privacy-safe diagnostics', () => {
  const url = new URL(
    buildAgentIssueUrl({
      actual: 'Agent repair did not resolve the issue.',
      envInfos: '> Chimera: 1.0.0',
      snapshot: projectPrivacySafeIssueSnapshot(snapshot),
    }),
  );

  assert.equal(url.origin, 'https://github.com');
  assert.equal(url.pathname, '/MFSGA/Chimera/issues/new');
  assert.equal(url.searchParams.get('template'), 'bug_report.yaml');
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

test('unsafe snapshots fail closed before issue projection', () => {
  assert.equal(
    projectPrivacySafeIssueSnapshot({
      ...snapshot,
      privacy: { ...safePrivacy, contains_raw_logs: true },
    }),
    null,
  );
});
