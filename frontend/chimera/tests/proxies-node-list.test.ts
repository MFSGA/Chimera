import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import type { NativeProxies_Serialize, ProxyGroup } from '@chimera/interface';
import {
  DEFAULT_NODE_VIEW,
  memberDelay,
  toNodeView,
  visibleMembers,
} from '../src/pages/(main)/main/proxies/_modules/node-list';

const group = {
  name: 'Automatic',
  type: 'URLTest',
  all: ['node10', 'node2', 'Failed', 'Untested', 'node1'],
  now: 'Failed',
  testUrl: 'https://latency.test/',
} as ProxyGroup;

const proxies = {
  groups: [group],
  nodes: {
    node10: { name: 'node10', type: 'vmess', history: [{ delay: 120 }] },
    node2: { name: 'node2', type: 'trojan', history: [{ delay: 35 }] },
    Failed: { name: 'Failed', type: 'socks5', history: [{ delay: 0 }] },
    Untested: { name: 'Untested', type: 'vless', history: [] },
    node1: { name: 'node1', type: 'vless', history: [{ delay: 35 }] },
  },
} as unknown as NativeProxies_Serialize;

describe('ref proxy group view over the native graph', () => {
  it('retains source order and matches all case-insensitive search terms', () => {
    assert.deepEqual(
      visibleMembers({ group, proxies, query: '', view: DEFAULT_NODE_VIEW }),
      group.all,
    );
    assert.deepEqual(
      visibleMembers({
        group,
        proxies,
        query: 'NODE 1',
        view: DEFAULT_NODE_VIEW,
      }),
      ['node10', 'node1'],
    );
    assert.deepEqual(
      visibleMembers({
        group,
        proxies,
        query: 'TROJAN node2',
        view: DEFAULT_NODE_VIEW,
      }),
      ['node2'],
    );
  });

  it('uses natural numeric ordering for node names', () => {
    assert.deepEqual(
      visibleMembers({
        group,
        proxies,
        query: '',
        view: { sort: 'name', hideUnavailable: false },
      }),
      ['Failed', 'node1', 'node2', 'node10', 'Untested'],
    );
  });

  it('sorts live delays before untested and failed and breaks ties in source order', () => {
    assert.deepEqual(
      visibleMembers({
        group,
        proxies,
        query: '',
        view: { sort: 'delay', hideUnavailable: false },
      }),
      ['node2', 'node1', 'node10', 'Untested', 'Failed'],
    );
  });

  it('never hides an untested or currently selected failed node', () => {
    assert.deepEqual(
      visibleMembers({
        group,
        proxies,
        query: '',
        view: { sort: 'default', hideUnavailable: true },
      }),
      group.all,
    );
    assert.deepEqual(
      visibleMembers({
        group: { ...group, now: 'node2' },
        proxies,
        query: '',
        view: { sort: 'default', hideUnavailable: true },
      }),
      group.all.filter((name) => name !== 'Failed'),
    );
  });

  it('prefers group-specific latency history if available', () => {
    const withSpecific = {
      ...proxies,
      nodes: {
        ...proxies.nodes,
        node2: {
          ...proxies.nodes.node2,
          extra: {
            [group.testUrl!]: { history: [{ delay: 99 }] },
          },
        },
      },
    } as NativeProxies_Serialize;
    assert.equal(memberDelay('node2', group, withSpecific), 99);
    assert.equal(memberDelay('Untested', group, proxies), undefined);
  });

  it('migrates invalid persisted view values to safe ref defaults', () => {
    assert.deepEqual(toNodeView(null), DEFAULT_NODE_VIEW);
    assert.deepEqual(
      toNodeView({ sort: 'made-up', hideUnavailable: 'yes' }),
      DEFAULT_NODE_VIEW,
    );
    assert.deepEqual(toNodeView({ sort: 'delay', hideUnavailable: true }), {
      sort: 'delay',
      hideUnavailable: true,
    });
  });
});
