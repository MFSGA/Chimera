import assert from 'node:assert/strict';
import test from 'node:test';
import { resolveProfileDetailState } from '../frontend/chimera/src/pages/(main)/main/profiles/$type/detail/_modules/profile-detail-state.ts';

const profiles = [
  { uid: 'profile-a', name: 'Alpha' },
  { uid: 'profile-b', name: 'Beta' },
];

test('profile detail remains loading before the first query result', () => {
  assert.deepEqual(resolveProfileDetailState(undefined, 'profile-a', true), {
    status: 'loading',
  });
});

test('profile detail reports missing after an empty query resolves', () => {
  assert.deepEqual(resolveProfileDetailState([], 'profile-a', false), {
    status: 'missing',
  });
});

test('profile detail finds an exact identifier', () => {
  assert.deepEqual(resolveProfileDetailState(profiles, 'profile-b', false), {
    status: 'found',
    profile: profiles[1],
  });
});

test('profile detail identifiers are case-sensitive', () => {
  assert.deepEqual(resolveProfileDetailState(profiles, 'PROFILE-A', false), {
    status: 'missing',
  });
});

test('profile detail rejects empty and encoded path-like identifiers', () => {
  for (const uid of ['', '../profile-a', 'profile%2Fa', 'profile/a']) {
    assert.deepEqual(resolveProfileDetailState(profiles, uid, false), {
      status: 'missing',
    });
  }
});

test('profile detail can use cached data while a refresh is pending', () => {
  assert.deepEqual(resolveProfileDetailState(profiles, 'profile-a', true), {
    status: 'found',
    profile: profiles[0],
  });
});

test('profile detail returns the first exact match for duplicate legacy data', () => {
  const duplicate = [
    { uid: 'duplicate', name: 'First' },
    { uid: 'duplicate', name: 'Second' },
  ];

  assert.deepEqual(resolveProfileDetailState(duplicate, 'duplicate', false), {
    status: 'found',
    profile: duplicate[0],
  });
});

test('profile detail reports missing when query data is absent after loading', () => {
  assert.deepEqual(resolveProfileDetailState(undefined, 'profile-a', false), {
    status: 'missing',
  });
});
