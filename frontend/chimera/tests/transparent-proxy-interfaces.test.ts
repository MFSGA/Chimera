import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { normalizeTransparentProxyInterfaces } from '../src/pages/(main)/main/settings/clash/_modules/transparent-proxy-interfaces';

describe('transparent proxy gateway interface normalization', () => {
  it('trims entries, removes empty values, and preserves first-seen order', () => {
    assert.deepEqual(
      normalizeTransparentProxyInterfaces(' wlan0, eth0, wlan0,  '),
      ['wlan0', 'eth0'],
    );
  });

  it('returns an empty list for an empty or whitespace-only value', () => {
    assert.deepEqual(normalizeTransparentProxyInterfaces(''), []);
    assert.deepEqual(normalizeTransparentProxyInterfaces(' ,  , '), []);
  });

  it('does not merge distinct interface names', () => {
    assert.deepEqual(
      normalizeTransparentProxyInterfaces('eth0, ETH0, enp2s0'),
      ['eth0', 'ETH0', 'enp2s0'],
    );
  });
});
