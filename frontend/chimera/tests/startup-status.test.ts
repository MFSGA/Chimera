import assert from 'node:assert/strict';
import test from 'node:test';
import { getStartupStatusView } from '../src/features/system-service/startup-status';

test('startup failure stays visible until explicitly dismissed', () => {
  assert.equal(getStartupStatusView('failed', false, false), 'failed');
  assert.equal(getStartupStatusView('failed', false, true), 'hidden');
});

test('a failed status query can also be dismissed', () => {
  assert.equal(getStartupStatusView(undefined, true, false), 'failed');
  assert.equal(getStartupStatusView(undefined, true, true), 'hidden');
});

test('a later startup failure is visible after dismissing a query error', () => {
  assert.equal(getStartupStatusView(undefined, true, true), 'hidden');
  assert.equal(getStartupStatusView('starting', false, true), 'loading');
  assert.equal(getStartupStatusView('failed', false, false), 'failed');
});

test('starting and ready states retain their expected presentation', () => {
  assert.equal(getStartupStatusView('starting', false, false), 'loading');
  assert.equal(getStartupStatusView('ready', false, false), 'hidden');
});
