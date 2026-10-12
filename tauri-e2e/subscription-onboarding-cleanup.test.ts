import assert from 'node:assert/strict';
import test from 'node:test';
import { planSubscriptionProfileCleanup as plan } from './subscription-onboarding-cleanup.ts';

const items = [
  { name: 'User profile', uid: 'user-1' },
  { name: 'test-unique', uid: 'owned-1' },
  { name: 'Unrelated', uid: 'user-2' },
];

test('partial setup without a snapshot does not change profiles', () => {
  assert.deepEqual(plan(undefined, 'user-1', items, 'test-unique'), {
    fixtureUids: [],
    restoreTo: undefined,
    originalMissing: false,
  });
});

test('absence of a test Profile does not affect current selection', () => {
  assert.deepEqual(plan('user-1', 'user-1', items, 'not-created'), {
    fixtureUids: [],
    restoreTo: undefined,
    originalMissing: false,
  });
});

test('active test Profile restores the original selection', () => {
  assert.deepEqual(plan('user-1', 'owned-1', items, 'test-unique'), {
    fixtureUids: ['owned-1'],
    restoreTo: 'user-1',
    originalMissing: false,
  });
});

test('null is a valid original selection', () => {
  assert.deepEqual(plan(null, 'owned-1', items, 'test-unique'), {
    fixtureUids: ['owned-1'],
    restoreTo: null,
    originalMissing: false,
  });
});

test('inactive test Profile does not replace an unrelated selection', () => {
  assert.deepEqual(plan('user-1', 'user-2', items, 'test-unique'), {
    fixtureUids: ['owned-1'],
    restoreTo: undefined,
    originalMissing: false,
  });
});

test('missing original Profile blocks unsafe restoration', () => {
  assert.deepEqual(plan('no-longer-present', 'owned-1', items, 'test-unique'), {
    fixtureUids: ['owned-1'],
    restoreTo: undefined,
    originalMissing: true,
  });
});
