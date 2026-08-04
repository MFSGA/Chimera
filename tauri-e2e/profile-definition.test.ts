import assert from 'node:assert/strict';
import test from 'node:test';
import type { ProfileResponse } from '../frontend/interface/src/ipc/bindings.ts';
import { remoteProfileDefinitionOf } from '../frontend/interface/src/ipc/profile-definition.ts';

const baseShared = {
  uid: 'profile-id',
  name: 'Profile Name',
  file: 'profile.yaml',
  desc: 'description',
  updated: 1_700_000_000,
};

test('local profile has no remote definition', () => {
  const profile: ProfileResponse = {
    type: 'local',
    ...baseShared,
    symlinks: null,
    chain: [],
  };

  assert.equal(remoteProfileDefinitionOf(profile), null);
});

test('remote profile definition preserves source options and subscription data', () => {
  const profile: ProfileResponse = {
    type: 'remote',
    ...baseShared,
    url: 'https://example.com/subscription.yaml',
    chain: ['transform-a'],
    option: {
      user_agent: 'Chimera/Test',
      with_proxy: true,
      self_proxy: false,
      update_interval_minutes: 30,
    },
    extra: {
      upload: 11,
      download: 22,
      total: 33,
      expire: 44,
    },
  };

  assert.deepEqual(remoteProfileDefinitionOf(profile), {
    type: 'config',
    config: {
      type: 'file',
      transforms: [],
      source: {
        type: 'remote',
        file: 'profile.yaml',
        updated_at: 1_700_000_000,
        url: 'https://example.com/subscription.yaml',
        option: {
          user_agent: 'Chimera/Test',
          with_proxy: true,
          self_proxy: false,
          update_interval_minutes: 30,
        },
        subscription: {
          upload: 11,
          download: 22,
          total: 33,
          expire: 44,
        },
      },
    },
  });
});

test('remote profile definition normalizes optional user agent and zero timestamp', () => {
  const profile: ProfileResponse = {
    type: 'remote',
    ...baseShared,
    updated: 0,
    url: 'https://example.com/empty.yaml',
    chain: [],
    option: {
      with_proxy: false,
      self_proxy: false,
      update_interval_minutes: 0,
    },
    extra: {
      upload: 0,
      download: 0,
      total: 0,
      expire: 0,
    },
  };

  const definition = remoteProfileDefinitionOf(profile);
  assert.ok(definition);
  assert.equal(definition.config.source.updated_at, null);
  assert.deepEqual(definition.config.source.option, {
    user_agent: null,
    with_proxy: false,
    self_proxy: false,
    update_interval_minutes: 0,
  });
});

test('remote profile definition conversion does not mutate its input', () => {
  const profile: ProfileResponse = {
    type: 'remote',
    ...baseShared,
    url: 'https://example.com/subscription.yaml',
    chain: [],
    option: {
      user_agent: null,
      with_proxy: false,
      self_proxy: true,
      update_interval_minutes: 60,
    },
    extra: {
      upload: 1,
      download: 2,
      total: 3,
      expire: 4,
    },
  };
  const snapshot = structuredClone(profile);

  remoteProfileDefinitionOf(profile);

  assert.deepEqual(profile, snapshot);
});
