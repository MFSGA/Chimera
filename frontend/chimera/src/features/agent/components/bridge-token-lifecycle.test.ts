import assert from 'node:assert/strict';
import test from 'node:test';
import {
  reduceBridgeToken,
  scheduleClipboardValueClear,
  scheduleTokenExpiry,
  TOKEN_CLIPBOARD_LIFETIME_MS,
  TOKEN_DISPLAY_LIFETIME_MS,
  type TimeoutScheduler,
} from './bridge-token-lifecycle';

class FakeScheduler implements TimeoutScheduler {
  private nextHandle = 1;
  readonly tasks = new Map<
    unknown,
    { callback: () => void | Promise<void>; delayMs: number }
  >();

  set(callback: () => void | Promise<void>, delayMs: number) {
    const handle = this.nextHandle++;
    this.tasks.set(handle, { callback, delayMs });
    return handle;
  }

  clear(handle: unknown) {
    this.tasks.delete(handle);
  }

  async runOnly() {
    assert.equal(this.tasks.size, 1);
    const [handle, task] = this.tasks.entries().next().value!;
    this.tasks.delete(handle);
    await task.callback();
  }
}

test('token reducer clears only the current one-time token', () => {
  let token = reduceBridgeToken(null, { type: 'started', token: 'first' });
  assert.equal(token, 'first');

  token = reduceBridgeToken(token, { type: 'started', token: 'second' });
  assert.equal(token, 'second');
  assert.equal(
    reduceBridgeToken(token, { type: 'expired', token: 'first' }),
    'second',
  );
  assert.equal(
    reduceBridgeToken(token, { type: 'copied', token: 'second' }),
    null,
  );
});

test('stopping the bridge clears the visible token immediately', () => {
  assert.equal(
    reduceBridgeToken('secret', { type: 'running_changed', running: false }),
    null,
  );
  assert.equal(
    reduceBridgeToken('secret', { type: 'running_changed', running: true }),
    'secret',
  );
});

test('display expiry is bounded and cancellable', async () => {
  const scheduler = new FakeScheduler();
  const expired: string[] = [];
  const cancel = scheduleTokenExpiry(
    'secret',
    (token) => expired.push(token),
    scheduler,
  );
  const task = scheduler.tasks.values().next().value!;

  assert.equal(task.delayMs, TOKEN_DISPLAY_LIFETIME_MS);
  cancel();
  assert.equal(scheduler.tasks.size, 0);

  scheduleTokenExpiry('secret', (token) => expired.push(token), scheduler);
  await scheduler.runOnly();
  assert.deepEqual(expired, ['secret']);
});

test('clipboard cleanup clears only an unchanged token value', async () => {
  const scheduler = new FakeScheduler();
  const writes: string[] = [];
  scheduleClipboardValueClear(
    'secret',
    {
      readText: async () => 'secret',
      writeText: async (value) => {
        writes.push(value);
      },
    },
    scheduler,
  );
  const task = scheduler.tasks.values().next().value!;

  assert.equal(task.delayMs, TOKEN_CLIPBOARD_LIFETIME_MS);
  await scheduler.runOnly();
  assert.deepEqual(writes, ['']);
});

test('clipboard cleanup preserves newer clipboard content and swallows failures', async () => {
  const changedScheduler = new FakeScheduler();
  const changedWrites: string[] = [];
  scheduleClipboardValueClear(
    'secret',
    {
      readText: async () => 'newer content',
      writeText: async (value) => {
        changedWrites.push(value);
      },
    },
    changedScheduler,
  );
  await changedScheduler.runOnly();
  assert.deepEqual(changedWrites, []);

  const failingScheduler = new FakeScheduler();
  scheduleClipboardValueClear(
    'secret',
    {
      readText: async () => {
        throw new Error('clipboard unavailable');
      },
      writeText: async () => {
        throw new Error('must not be called');
      },
    },
    failingScheduler,
  );
  await assert.doesNotReject(() => failingScheduler.runOnly());
});
