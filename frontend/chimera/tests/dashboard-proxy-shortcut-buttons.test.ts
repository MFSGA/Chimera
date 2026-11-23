import assert from 'node:assert/strict';
import test from 'node:test';
import {
  proxyShortcutPreviewStatus,
  proxyShortcutSelectionSummary,
  toggleProxyShortcutSelection,
  visibleProxyShortcutButtons,
  visibleSelectedProxyShortcutButtons,
} from '../src/features/dashboard/proxy-shortcut-buttons.ts';

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

const labels = {
  all: 'All',
  system: 'System proxy',
  tun: 'TUN',
  transparent: 'Transparent capture',
};

test('ref config summary names the actual visible shortcuts, not their raw count', () => {
  const all = ['system', 'tun', 'transparent'] as const;
  assert.equal(proxyShortcutSelectionSummary(all, true, labels), 'All');
  assert.equal(proxyShortcutSelectionSummary(all, false, labels), 'All');
  assert.equal(
    proxyShortcutSelectionSummary(['tun', 'transparent'], false, labels),
    'TUN',
  );
  assert.equal(
    proxyShortcutSelectionSummary(['transparent', 'system'], true, labels),
    'Transparent capture, System proxy',
  );
  assert.equal(
    proxyShortcutSelectionSummary(['transparent', 'system'], false, labels),
    'System proxy',
  );
  assert.equal(proxyShortcutSelectionSummary([], true, labels), '—');
});

test('cross-platform configuration retains saved Linux-only selection', () => {
  const stored = ['transparent', 'system', 'tun'] as const;
  assert.deepEqual(visibleSelectedProxyShortcutButtons(stored, false), [
    'system',
    'tun',
  ]);
  const changed = toggleProxyShortcutSelection(stored, 'tun', false, false);
  assert.deepEqual(changed, ['transparent', 'system']);
  assert.deepEqual(visibleSelectedProxyShortcutButtons(changed, true), [
    'transparent',
    'system',
  ]);
  assert.deepEqual(visibleSelectedProxyShortcutButtons(changed, false), [
    'system',
  ]);
});

test('last visible shortcut cannot be removed, even with hidden selections', () => {
  const stored = ['transparent', 'system'] as const;
  assert.deepEqual(
    toggleProxyShortcutSelection(stored, 'system', false, false),
    stored,
  );
  assert.deepEqual(
    toggleProxyShortcutSelection(stored, 'transparent', false, false),
    stored,
  );
  assert.deepEqual(
    toggleProxyShortcutSelection(['system'], 'system', false, true),
    ['system'],
  );
  assert.deepEqual(toggleProxyShortcutSelection(['tun'], 'tun', false, false), [
    'tun',
  ]);
  assert.deepEqual(
    toggleProxyShortcutSelection(['transparent'], 'system', true, false),
    ['transparent', 'system'],
  );
  assert.deepEqual(
    toggleProxyShortcutSelection(
      ['transparent', 'system'],
      'system',
      false,
      true,
    ),
    ['transparent'],
  );
});

test('checking already selected shortcuts is idempotent', () => {
  const selected = ['system', 'tun'] as const;
  assert.deepEqual(
    toggleProxyShortcutSelection(selected, 'tun', true, false),
    selected,
  );
  assert.deepEqual(
    toggleProxyShortcutSelection(selected, 'system', false, false),
    ['tun'],
  );
  assert.deepEqual(selected, ['system', 'tun']);
});

test('proxy widget drag preview shows only supported controls on this host', () => {
  assert.equal(proxyShortcutPreviewStatus([]), 'disabled');
  assert.equal(
    proxyShortcutPreviewStatus(
      visibleProxyShortcutButtons(['transparent'], 'system-first', false),
    ),
    'disabled',
  );
  assert.equal(
    proxyShortcutPreviewStatus(
      visibleProxyShortcutButtons(
        ['transparent', 'tun'],
        'system-first',
        false,
      ),
    ),
    'tun',
  );
  assert.equal(
    proxyShortcutPreviewStatus(
      visibleProxyShortcutButtons(['transparent'], 'system-first', true),
    ),
    'transparent',
  );
  assert.equal(
    proxyShortcutPreviewStatus(
      visibleProxyShortcutButtons(['tun', 'system'], 'tun-first', false),
    ),
    'system',
  );
});
