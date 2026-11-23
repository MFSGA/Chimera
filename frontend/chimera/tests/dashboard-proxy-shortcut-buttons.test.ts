import assert from 'node:assert/strict';
import test from 'node:test';
import { visibleProxyShortcutButtons } from '../src/features/dashboard/proxy-shortcut-buttons.ts';

test('ref proxy shortcut order is identical for live and preview rendering', () => {
  const all = ['system', 'tun', 'transparent'] as const;
  assert.deepEqual(visibleProxyShortcutButtons(all, 'system-first', true), all);
  assert.deepEqual(visibleProxyShortcutButtons(all, 'tun-first', true), [
    'tun',
    'system',
    'transparent',
  ]);
  assert.deepEqual(visibleProxyShortcutButtons(all, 'tun-first', false), [
    'tun',
    'system',
  ]);
});

test('unselected proxy buttons never render; transparent capture is Linux-only', () => {
  assert.deepEqual(visibleProxyShortcutButtons(['tun'], 'system-first', true), [
    'tun',
  ]);
  assert.deepEqual(
    visibleProxyShortcutButtons(['transparent'], 'system-first', false),
    [],
  );
  assert.deepEqual(
    visibleProxyShortcutButtons(['transparent'], 'system-first', true),
    ['transparent'],
  );
  assert.deepEqual(visibleProxyShortcutButtons([], 'system-first', true), []);
});
