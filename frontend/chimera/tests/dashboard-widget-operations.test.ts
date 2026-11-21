import assert from 'node:assert/strict';
import test from 'node:test';
import { createWidgetOperationGuard } from '../src/features/dashboard/widget-operation-guard.ts';
import { isWidgetQueryReconciled } from '../src/features/dashboard/widget-query-reconciliation.ts';

const deferred = () => {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
};

test('duplicate provider clicks across widget instances share one synchronous lock', async () => {
  const guard = createWidgetOperationGuard();
  const waiting = deferred();
  const calls: string[] = [];
  const update = async () => {
    calls.push('proxy:regional');
    await waiting.promise;
  };
  const first = guard.run('proxy:regional', update);
  assert.equal(guard.isPending('proxy:regional'), true);
  assert.equal(await guard.run('proxy:regional', update), false);
  assert.deepEqual(calls, ['proxy:regional']);
  waiting.release();
  assert.equal(await first, true);
  assert.equal(guard.isPending('proxy:regional'), false);
  assert.equal(await guard.run('proxy:regional', async () => {}), true);
});

test('proxy and rule providers with the same name can update independently', async () => {
  const guard = createWidgetOperationGuard();
  const proxy = deferred();
  const rule = deferred();
  const calls: string[] = [];
  const first = guard.run('proxy:regional', async () => {
    calls.push('proxy');
    await proxy.promise;
  });
  const second = guard.run('rule:regional', async () => {
    calls.push('rule');
    await rule.promise;
  });
  assert.deepEqual(calls, ['proxy', 'rule']);
  assert.equal(guard.isPending('proxy:regional'), true);
  assert.equal(guard.isPending('rule:regional'), true);
  proxy.release();
  assert.equal(await first, true);
  assert.equal(guard.isPending('rule:regional'), true);
  rule.release();
  assert.equal(await second, true);
});

test('a rejected backend update unlocks the provider for a later retry', async () => {
  const guard = createWidgetOperationGuard();
  await assert.rejects(
    guard.run('proxy:primary', async () => {
      throw new Error('unconfirmed');
    }),
    /unconfirmed/,
  );
  assert.equal(guard.isPending('proxy:primary'), false);
  assert.equal(await guard.run('proxy:primary', async () => {}), true);
});

test('reconciliation remains locked while a failed mutation rechecks its result', async () => {
  const guard = createWidgetOperationGuard();
  const recheck = deferred();
  const first = guard.run('rule:regional', async () => {
    try {
      throw new Error('lost mutation reply');
    } catch {
      await recheck.promise;
    }
  });
  assert.equal(guard.isPending('rule:regional'), true);
  assert.equal(await guard.run('rule:regional', async () => {}), false);
  recheck.release();
  assert.equal(await first, true);
  assert.equal(guard.isPending('rule:regional'), false);
});

test('successful query reconciliation requires a non-null fresh result', () => {
  assert.equal(isWidgetQueryReconciled({ isError: false, data: {} }), true);
  assert.equal(isWidgetQueryReconciled({ isError: false, data: [] }), true);
  assert.equal(isWidgetQueryReconciled({ isError: false, data: null }), false);
  assert.equal(
    isWidgetQueryReconciled({ isError: false, data: undefined }),
    false,
  );
});

test('an error result is not reconciled even when stale data remains cached', () => {
  assert.equal(
    isWidgetQueryReconciled({ isError: true, data: { mode: 'global' } }),
    false,
  );
  assert.equal(isWidgetQueryReconciled({ isError: true, data: {} }), false);
});
