import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  normalizeTrafficViewSettings,
  type TrafficViewSettings,
} from '../src/pages/(main)/main/topology/_modules/view-settings-model';

const rankingOrder = [
  'process',
  'target',
  'exit',
  'source',
  'inbound',
] as const;
const defaults: TrafficViewSettings<(typeof rankingOrder)[number]> = {
  rankings: ['process', 'target', 'exit'],
  flowGraph: 'sankey',
};
const migrate = (value: unknown) =>
  normalizeTrafficViewSettings(value, rankingOrder, defaults);

describe('reference traffic view settings migration', () => {
  it('defaults to the reference ranking order and Sankey graph', () => {
    assert.deepEqual(migrate(undefined), defaults);
  });

  it('filters unknown and duplicate rankings while preserving canonical order', () => {
    assert.deepEqual(
      migrate({
        rankings: ['inbound', 'unknown', 'process', 'inbound'],
        flowGraph: 'cards',
      }),
      { rankings: ['process', 'inbound'], flowGraph: 'cards' },
    );
  });

  it('repairs invalid stored values without discarding valid preferences', () => {
    assert.deepEqual(migrate({ rankings: ['target'], flowGraph: 'unknown' }), {
      rankings: ['target'],
      flowGraph: 'sankey',
    });
    assert.deepEqual(migrate({ rankings: 'invalid' }), {
      rankings: ['process', 'target', 'exit'],
      flowGraph: 'sankey',
    });
  });
});
