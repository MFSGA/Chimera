import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import {
  toQuery,
  toTopologyRequest,
  trafficSearchSchema,
} from '../src/pages/(main)/main/topology/_modules/search';

describe('reference traffic analysis URL state', () => {
  it('uses stable defaults for a new traffic page URL', () => {
    assert.deepEqual(trafficSearchSchema.parse({}), {
      range: 'last_hour',
      scope: 'all',
      filters: [],
      metric: 'bytes',
      limit: 7,
    });
  });

  it('converts URL filters into the shared traffic query contract', () => {
    const search = trafficSearchSchema.parse({
      range: 'last7_days',
      scope: 'closed',
      filters: [
        { d: 'rule', v: 'DOMAIN-SUFFIX,example.test' },
        { d: 'destination_region', v: 'JP' },
      ],
    });

    assert.deepEqual(toQuery(search), {
      range: 'last7_days',
      scope: 'closed',
      filters: [
        { dimension: 'rule', value: 'DOMAIN-SUFFIX,example.test' },
        { dimension: 'destination_region', value: 'JP' },
      ],
    });
  });

  it('requests topology only for views that render it', () => {
    assert.equal(toTopologyRequest({ view: 'overview', limit: 7 }), null);
    assert.deepEqual(
      toTopologyRequest({ view: 'flow', layers: 'inbound', limit: 10 }),
      {
        layers: ['inbound', 'rule', 'exit'],
        limit_per_layer: 10,
      },
    );
  });

  it('keeps every geographic region and the lookup basis in the map request', () => {
    assert.deepEqual(toTopologyRequest({ view: 'map', limit: 5 }), {
      layers: ['source_region', 'destination_region', 'destination_basis'],
      limit_per_layer: null,
    });
  });

  it('maps the flow “all” limit to an unbounded backend request', () => {
    assert.deepEqual(
      toTopologyRequest({ view: 'flow', layers: 'source', limit: 'all' }),
      {
        layers: ['source', 'target', 'exit'],
        limit_per_layer: null,
      },
    );
  });
});
