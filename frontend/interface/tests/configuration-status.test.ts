import assert from 'node:assert/strict';
import test from 'node:test';
import type { ConfigurationStatus, SourceStatus } from '../src/ipc/bindings.js';
import {
  acceptConfigurationStatus,
  attentionSources,
  sourceMessage,
} from '../src/ipc/configuration-status.js';

const status = (
  event_seq: number,
  health: ConfigurationStatus['runtime']['health'],
): ConfigurationStatus => ({
  event_seq,
  maintenance: null,
  runtime: {
    health,
    operation_id: null,
    message: null,
  },
  source_versions: { application: 1, clash: 1, session: 1, profiles: 1 },
  effects: [],
  sources: [],
  recent_operations: [],
  active: null,
});
const source = (
  profile: string,
  outcome: SourceStatus['outcome'],
  health: SourceStatus['health'],
): SourceStatus => ({
  profile,
  name: profile.toUpperCase(),
  origin: 'scheduled_refresh',
  outcome,
  health,
  at: 0,
});
test('keeps a newer failure when an old success arrives late', () => {
  const blocked = status(12, 'blocked');
  assert.equal(
    acceptConfigurationStatus(blocked, status(11, 'healthy')),
    blocked,
  );
  assert.equal(
    acceptConfigurationStatus(blocked, status(12, 'healthy')),
    blocked,
  );
  assert.equal(
    acceptConfigurationStatus(blocked, status(13, 'healthy')).runtime.health,
    'healthy',
  );
});
test('shows only background sources that need attention', () => {
  const failed = source(
    'failed',
    { kind: 'failed', message: 'download failed' },
    'blocked',
  );
  const rejected = source(
    'rejected',
    {
      kind: 'rejected',
      code: 'external_source_rejected',
      message: 'not a mapping',
    },
    'blocked',
  );
  const withSources = {
    ...status(1, 'healthy'),
    sources: [
      source('committed', { kind: 'committed', operation_id: 'op' }, 'healthy'),
      failed,
      source(
        'superseded',
        { kind: 'superseded', reason: 'changed' },
        'healthy',
      ),
      rejected,
    ],
  };
  assert.deepEqual(attentionSources(withSources), [failed, rejected]);
  assert.deepEqual(attentionSources(status(1, 'healthy')), []);
  assert.deepEqual(attentionSources(withSources).map(sourceMessage), [
    'download failed',
    'not a mapping',
  ]);
});
