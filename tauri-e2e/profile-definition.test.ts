import assert from 'node:assert/strict';
import test from 'node:test';
import type { ProfileItem_Serialize } from '../frontend/interface/src/ipc/bindings.js';
import { remoteProfileDefinitionOf } from '../frontend/interface/src/ipc/profile-definition.ts';

// Pure unit contract: map the committed typed Profile item into a remote
// definition for editing, preserving source data and scoped transforms.
const baseItem = {
  uid: 'profile-id',
  name: 'Profile Name',
  desc: 'description',
} satisfies Pick<ProfileItem_Serialize, 'uid' | 'name' | 'desc'>;

test('local Profile items do not produce a remote definition', () => {
  const profile: ProfileItem_Serialize = {
    ...baseItem,
    type: 'config',
    config: {
      type: 'file',
      transforms: [],
      source: {
        type: 'local',
        binding: {
          type: 'managed',
          file: 'profile.yaml',
        },
      },
    },
  };

  assert.equal(remoteProfileDefinitionOf(profile), null);
});

test('remote Profile definition preserves source options, subscription data, and scoped transforms', () => {
  const profile: ProfileItem_Serialize = {
    ...baseItem,
    type: 'config',
    config: {
      type: 'file',
      transforms: ['transform-a'],
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
  };

  assert.deepEqual(remoteProfileDefinitionOf(profile), {
    type: 'config',
    config: {
      type: 'file',
      transforms: ['transform-a'],
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

test('remote definition conversion preserves zero timestamps and leaves the item unchanged', () => {
  const profile: ProfileItem_Serialize = {
    ...baseItem,
    type: 'config',
    config: {
      type: 'file',
      transforms: [],
      source: {
        type: 'remote',
        file: 'profile.yaml',
        updated_at: 0,
        url: 'https://example.com/empty.yaml',
        option: {
          with_proxy: false,
          self_proxy: false,
          update_interval_minutes: 0,
        },
        subscription: { upload: 0, download: 0, total: 0, expire: 0 },
      },
    },
  };
  const snapshot = structuredClone(profile);

  const definition = remoteProfileDefinitionOf(profile);

  assert.ok(definition);
  assert.equal(
    definition.type === 'config' && definition.config.type === 'file'
      ? definition.config.source.updated_at
      : undefined,
    0,
  );
  assert.deepEqual(profile, snapshot);
});
