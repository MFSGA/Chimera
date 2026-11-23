import assert from 'node:assert/strict';
import test from 'node:test';
import { runServiceOperation } from './service-operation';

test('OS Service success and ref Core reconcile success are distinct completed steps', async () => {
  const events: string[] = [];
  const result = await runServiceOperation(
    'start',
    async (operation) => {
      events.push(`service:${operation}`);
    },
    async () => {
      events.push('core:reconcile');
    },
  );
  assert.deepEqual(result, { stage: 'completed' });
  assert.deepEqual(events, ['service:start', 'core:reconcile']);
});

test('an OS Service command error stops before attempting Core restart', async () => {
  const failure = new Error('service elevation denied');
  let coreRestarts = 0;
  const result = await runServiceOperation(
    'install',
    async () => {
      throw failure;
    },
    async () => {
      coreRestarts += 1;
    },
  );
  assert.deepEqual(result, { stage: 'service_failed', error: failure });
  assert.equal(coreRestarts, 0);
});

test('a Core reconcile error cannot turn a completed OS Service stop into a Service failure', async () => {
  const events: string[] = [];
  const failure = new Error('CoreActor revision changed during rebuild');
  const result = await runServiceOperation(
    'stop',
    async (operation) => {
      events.push(`service:${operation}`);
    },
    async () => {
      events.push('core:reconcile');
      throw failure;
    },
  );
  assert.deepEqual(result, { stage: 'core_restart_failed', error: failure });
  assert.deepEqual(events, ['service:stop', 'core:reconcile']);
});
