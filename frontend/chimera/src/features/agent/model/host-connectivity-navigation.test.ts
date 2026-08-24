import assert from 'node:assert/strict';
import test from 'node:test';
import { inspectHostConnectivityCard } from './host-connectivity-navigation';

test('host connectivity inspection refreshes before focusing the existing card', async () => {
  const events: string[] = [];
  let scheduled: (() => void) | null = null;
  const card = {
    focus: () => events.push('focus'),
    scrollIntoView: () => events.push('scroll'),
  } as unknown as HTMLElement;

  await inspectHostConnectivityCard({
    refresh: async () => {
      events.push('refresh');
    },
    schedule: (callback) => {
      events.push('schedule');
      scheduled = callback;
    },
    findCard: () => card,
  });

  assert.deepEqual(events, ['refresh', 'schedule']);
  const callback = scheduled as unknown as () => void;
  callback();
  assert.deepEqual(events, ['refresh', 'schedule', 'focus', 'scroll']);
});

test('host connectivity inspection safely tolerates a missing card', async () => {
  let scheduled: (() => void) | null = null;

  await inspectHostConnectivityCard({
    refresh: async () => undefined,
    schedule: (callback) => {
      scheduled = callback;
    },
    findCard: () => null,
  });

  assert.doesNotThrow(() => scheduled?.());
});
