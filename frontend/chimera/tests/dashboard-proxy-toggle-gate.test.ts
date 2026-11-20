import assert from 'node:assert/strict';
import test from 'node:test';
import { createProxyToggleGate } from '../src/features/system-proxy/proxy-toggle-gate.ts';

function deferred() {
  let release!: () => void;
  const promise = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { promise, release };
}

test('settings unavailable: do not write or pretend to toggle', async () => {
  const gate = createProxyToggleGate();
  let writes = 0;
  const result = await gate.run(
    'tun',
    undefined,
    async () => {
      writes++;
    },
    async () => {},
  );
  assert.equal(result, false);
  assert.equal(writes, 0);
});

test('repeated clicks across dashboard/settings instances share one write and refresh', async () => {
  const gate = createProxyToggleGate();
  const commitStarted = deferred();
  const allowCommit = deferred();
  const refreshStarted = deferred();
  const allowRefresh = deferred();
  const writes: boolean[] = [];
  let refreshes = 0;
  const commit = async (value: boolean) => {
    writes.push(value);
    commitStarted.release();
    await allowCommit.promise;
  };
  const refresh = async () => {
    refreshes++;
    refreshStarted.release();
    await allowRefresh.promise;
  };
  const first = gate.run('system', false, commit, refresh);
  await commitStarted.promise;
  assert.equal(await gate.run('system', false, commit, refresh), false);
  allowCommit.release();
  await refreshStarted.promise;
  assert.equal(await gate.run('system', false, commit, refresh), false);
  allowRefresh.release();
  assert.equal(await first, true);
  assert.deepEqual(writes, [true]);
  assert.equal(refreshes, 1);
  assert.equal(await gate.run('system', true, commit, refresh), true);
  assert.deepEqual(writes, [true, false]);
});

test('system and TUN are separate actions; an error unlocks the failed kind', async () => {
  const gate = createProxyToggleGate();
  const systemHold = deferred();
  const systemFirst = gate.run(
    'system',
    false,
    async () => {
      await systemHold.promise;
    },
    async () => {},
  );
  const tunWrites: boolean[] = [];
  assert.equal(
    await gate.run(
      'tun',
      true,
      async (v) => {
        tunWrites.push(v);
      },
      async () => {},
    ),
    true,
  );
  assert.deepEqual(tunWrites, [false]);
  systemHold.release();
  await systemFirst;
  await assert.rejects(
    gate.run(
      'tun',
      false,
      async () => {
        throw new Error('backend rejected');
      },
      async () => {},
    ),
    /backend rejected/,
  );
  assert.equal(
    await gate.run(
      'tun',
      false,
      async () => {},
      async () => {},
    ),
    true,
  );
});

test('a refresh failure does not leak the lock or retry the persisted write', async () => {
  const gate = createProxyToggleGate();
  let writes = 0;
  await assert.rejects(
    gate.run(
      'tun',
      false,
      async () => {
        writes++;
      },
      async () => {
        throw new Error('query offline');
      },
    ),
    /query offline/,
  );
  assert.equal(writes, 1);
  assert.equal(
    await gate.run(
      'tun',
      true,
      async () => {
        writes++;
      },
      async () => {},
    ),
    true,
  );
  assert.equal(writes, 2);
});
