import assert from 'node:assert/strict';
import test from 'node:test';
import { runProviderAction } from '../src/pages/(main)/main/providers/_modules/provider-action';
import { formatRelativeTime } from '../src/utils/date';

test('a completed native provider mutation resolves without a failure notice', async () => {
  let called = false;
  const value = await runProviderAction(
    async () => ({ updated: true }),
    () => {
      called = true;
    },
  );
  assert.deepEqual(value, { updated: true });
  assert.equal(called, false);
});

test('a failed provider mutation is reported exactly once and remains rejected', async () => {
  const reason = new Error('native update rejected');
  const reports: unknown[] = [];
  await assert.rejects(
    runProviderAction(
      async () => {
        throw reason;
      },
      async (error) => {
        reports.push(error);
      },
    ),
    (caught) => caught === reason,
  );
  assert.deepEqual(reports, [reason]);
});

test('reporting failure must not turn a rejected provider mutation into success', async () => {
  const reason = new Error('provider request rejected');
  const notificationFailure = new Error('dialog unavailable');
  await assert.rejects(
    runProviderAction(
      async () => {
        throw reason;
      },
      async () => {
        throw notificationFailure;
      },
    ),
    (caught) => caught === reason,
  );
});

test('provider relative timestamps respect the selected locale and handle missing data', () => {
  const now = Date.UTC(2026, 10, 20, 12);
  const hourAgo = now - 60 * 60 * 1000;
  assert.match(formatRelativeTime(hourAgo, now, 'en'), /hour/);
  assert.match(formatRelativeTime(hourAgo, now, 'zh-cn'), /小时/);
  assert.equal(formatRelativeTime(null, now, 'en'), '-');
});
