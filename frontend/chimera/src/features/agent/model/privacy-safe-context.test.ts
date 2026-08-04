import assert from 'node:assert/strict';
import test from 'node:test';
import type { AgentNetworkSnapshot } from '@chimera/interface';
import {
  isPrivacySafeSnapshot,
  projectPrivacySafeIssueSnapshot,
  serializePrivacySafeSnapshot,
} from './privacy-safe-context';

const snapshot = {
  privacy: {
    contains_raw_logs: false,
    contains_profile_names: false,
    contains_profile_urls: false,
    contains_connection_targets: false,
    contains_controller_secret: false,
  },
} as AgentNetworkSnapshot;

test('complete snapshot is available only when every privacy assertion is false', () => {
  assert.equal(isPrivacySafeSnapshot(snapshot), true);
  assert.match(serializePrivacySafeSnapshot(snapshot) ?? '', /"privacy"/);
});

test('Issue projection contains only stable diagnostic fields', () => {
  const projected = projectPrivacySafeIssueSnapshot(snapshot);
  assert.deepEqual(projected, {
    schema_version: snapshot.schema_version,
    health: snapshot.health,
    findings: snapshot.findings,
    probe_failures: snapshot.probe_failures,
  });
  assert.equal('privacy' in (projected ?? {}), false);
});

test('complete snapshot fails closed when any privacy assertion is true', () => {
  for (const key of Object.keys(snapshot.privacy) as Array<
    keyof AgentNetworkSnapshot['privacy']
  >) {
    const unsafe = {
      ...snapshot,
      privacy: { ...snapshot.privacy, [key]: true },
    };
    assert.equal(isPrivacySafeSnapshot(unsafe), false, key);
    assert.equal(serializePrivacySafeSnapshot(unsafe), null, key);
    assert.equal(projectPrivacySafeIssueSnapshot(unsafe), null, key);
  }
});
