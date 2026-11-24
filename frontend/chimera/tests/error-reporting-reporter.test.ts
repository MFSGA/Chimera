import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { FrontendEventBatch } from '@chimera/interface';
import { fromConsole } from '../src/services/error-reporting/normalize';
import { createReporter } from '../src/services/error-reporting/reporter';

/** A manual clock and timer queue; `advance` runs due timers in order. */
function harness(send?: (batch: FrontendEventBatch) => Promise<void>) {
  let time = 1_000_000;
  let timers: { at: number; callback: () => void; id: number }[] = [];
  let nextId = 0;
  const batches: FrontendEventBatch[] = [];

  const reporter = createReporter({
    send:
      send ??
      (async (batch) => {
        batches.push(batch);
      }),
    now: () => time,
    route: () => '/main/dashboard',
    schedule: (callback, ms) => {
      const id = nextId++;
      timers.push({ at: time + ms, callback, id });
      return () => {
        timers = timers.filter((timer) => timer.id !== id);
      };
    },
  });

  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

  const advance = async (ms: number) => {
    const target = time + ms;
    for (;;) {
      const due = timers
        .filter((timer) => timer.at <= target)
        .sort((a, b) => a.at - b.at)[0];
      if (!due) break;
      timers = timers.filter((timer) => timer !== due);
      time = due.at;
      due.callback();
    }
    time = target;
    await settle();
  };

  return {
    reporter,
    batches,
    advance,
    settle,
    pendingTimers: () => timers.length,
  };
}

const error = (message: string) =>
  fromConsole('error', [message, new Error(message)]);

test('repeats within one batch are merged into one counted event', async () => {
  const { reporter, batches, advance } = harness();

  for (let i = 0; i < 100; i++) reporter.capture(error('boom'));
  await advance(1_000);

  assert.equal(batches.length, 1);
  assert.equal(batches[0].dropped, 0);
  assert.equal(batches[0].events.length, 1);
  assert.partialDeepStrictEqual(batches[0].events[0], {
    message: 'boom Error: boom',
    count: 100,
    route: '/main/dashboard',
    first_seen_ms: 1_000_000,
  });
});

test('a repeat after the first send is held until its window ends', async () => {
  const { reporter, batches, advance } = harness();

  reporter.capture(error('boom'));
  await advance(1_000);
  reporter.capture(error('boom'));
  reporter.capture(error('boom'));
  await advance(30_000);
  assert.equal(batches.length, 1);

  await advance(30_000);
  assert.equal(batches.length, 2);
  assert.equal(batches[1].events[0].count, 2);

  // The window ended: the next occurrence is reported promptly again.
  reporter.capture(error('boom'));
  await advance(1_000);
  assert.equal(batches.length, 3);
  assert.equal(batches[2].events[0].count, 1);
});

test('a full batch is sent at once', async () => {
  const { reporter, batches, settle } = harness();

  for (let i = 0; i < 16; i++) {
    reporter.capture(error(`distinct ${'x'.repeat(i)}`));
  }
  await settle();

  assert.equal(batches.length, 1);
  assert.equal(batches[0].events.length, 16);
});

test('distinct events beyond the rate limit are counted as dropped', async () => {
  const { reporter, batches, advance } = harness();

  for (let i = 0; i < 200; i++) reporter.capture(error(`e${'x'.repeat(i)}`));
  await advance(1_000);

  const sent = batches.flatMap((batch) => batch.events);
  const dropped = batches.reduce((sum, batch) => sum + batch.dropped, 0);
  assert.equal(sent.length, 60);
  assert.equal(dropped, 140);

  await advance(60_000);
  reporter.capture(error('after the window'));
  await advance(1_000);
  assert.ok(batches.at(-1)?.events[0].message?.includes('after the window'));
});

test('a failed send is not retried and is reported as dropped', async () => {
  let fail = true;
  const batches: FrontendEventBatch[] = [];
  const { reporter, advance } = harness(async (batch) => {
    if (fail) throw new Error('offline');
    batches.push(batch);
  });

  reporter.capture(error('a'));
  reporter.capture(error('b'));
  await advance(1_000);
  assert.equal(batches.length, 0);

  fail = false;
  reporter.capture(error('c'));
  await advance(1_000);
  assert.equal(batches.length, 1);
  assert.deepEqual(
    batches[0].events.map((event) => event.message),
    ['c Error: c'],
  );
  assert.equal(batches[0].dropped, 2);
});

test('logging done while sending is not captured', async () => {
  const batches: FrontendEventBatch[] = [];
  let reporterRef: ReturnType<typeof createReporter> | null = null;
  const { reporter, advance } = harness(async (batch) => {
    reporterRef?.capture(error('from the transport'));
    batches.push(batch);
  });
  reporterRef = reporter;

  reporter.capture(error('original'));
  await advance(5_000);

  assert.equal(batches.length, 1);
  assert.deepEqual(
    batches[0].events.map((event) => event.message),
    ['original Error: original'],
  );
});

test('dispose sends what is queued and stops the timer', async () => {
  const { reporter, batches, settle, pendingTimers } = harness();

  reporter.capture(error('last words'));
  reporter.dispose();
  await settle();

  assert.equal(batches.length, 1);
  assert.equal(pendingTimers(), 0);
});
