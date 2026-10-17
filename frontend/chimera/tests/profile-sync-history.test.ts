import assert from 'node:assert/strict';
import test from 'node:test';
import {
  didHistoryRevisionAdvance,
  didScheduledRunAdvance,
} from '../../interface/src/ipc/use-profile-sync';
import { hasDegradedEffects } from '../src/pages/(main)/main/profiles/$type/detail/_modules/sync-history-utils';

test('recognizes a run summary with degraded effects', () => {
  assert.equal(hasDegradedEffects('{"degradation_count":2}'), true);
});

test('does not flag clean or malformed retained output', () => {
  assert.equal(hasDegradedEffects('{"degradation_count":0}'), false);
  assert.equal(hasDegradedEffects('{"degradation_count":"2"}'), false);
  assert.equal(hasDegradedEffects('{broken journal output'), false);
});

test('detects a scheduled run that completed between status polls', () => {
  const due = '2026-10-16T12:00:00Z';
  const advanced = '2026-10-16T12:05:00Z';

  assert.equal(
    didScheduledRunAdvance(due, advanced, Date.parse('2026-10-16T12:00:01Z')),
    true,
  );
});

test('detects a run completed outside the current WebView', () => {
  assert.equal(didHistoryRevisionAdvance(undefined, '1'), true);
  assert.equal(didHistoryRevisionAdvance(undefined, null), false);
  assert.equal(didHistoryRevisionAdvance('17', '18'), true);
  assert.equal(didHistoryRevisionAdvance(null, '1'), true);
  assert.equal(didHistoryRevisionAdvance('18', '18'), false);
});

test('ignores future, unchanged, and invalid schedule timestamps', () => {
  const future = '2026-10-16T12:05:00Z';
  assert.equal(
    didScheduledRunAdvance(
      future,
      '2026-10-16T12:10:00Z',
      Date.parse('2026-10-16T12:00:00Z'),
    ),
    false,
  );
  assert.equal(
    didScheduledRunAdvance(future, future, Date.parse(future)),
    false,
  );
  assert.equal(
    didScheduledRunAdvance('invalid', future, Date.parse(future)),
    false,
  );
});
