import assert from 'node:assert/strict';
import test from 'node:test';
import type { AgentAutonomyPolicyStatus } from '@chimera/interface';
import { presentAutonomyStatus } from './autonomy-status';

test('terminal autonomy states remain distinguishable', () => {
  assert.equal(presentAutonomyStatus('expired'), 'expired');
  assert.equal(presentAutonomyStatus('revoked'), 'revoked');
  assert.equal(presentAutonomyStatus('session_mismatch'), 'session_mismatch');
});

test('active and rejected policy states map conservatively', () => {
  assert.equal(presentAutonomyStatus('active'), 'active');
  for (const status of [
    undefined,
    'disabled',
    'schema_version_mismatch',
    'scope_mismatch',
    'empty_allowlist',
    'duration_out_of_range',
    'action_budget_out_of_range',
    'action_not_allowed',
    'action_budget_exhausted',
    'action_in_flight',
  ] satisfies Array<AgentAutonomyPolicyStatus | undefined>) {
    assert.equal(presentAutonomyStatus(status), 'inactive');
  }
});
