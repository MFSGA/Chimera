import assert from 'node:assert/strict';
import test from 'node:test';
import { commands, type ProfileLayout } from '../src/ipc/bindings.js';
import { mutations } from '../src/ipc/profile-bindings.js';
import {
  invokeMutation,
  MutationUnconfirmedError,
} from '../src/ipc/query-options.js';
import { unwrapResult } from '../src/utils/index.js';

const layout: ProfileLayout = {
  folders: [{ id: 'f1', name: 'Work', members: ['a'] }],
  ungrouped: ['b'],
};

test('setProfileLayout passes an entire layout through the generated command once', async () => {
  const original = commands.setProfileLayout;
  const calls: ProfileLayout[] = [];
  commands.setProfileLayout = async (value) => {
    calls.push(value);
    return {
      status: 'ok',
      data: {
        status: 'committed',
        value: null,
        commits: [],
        notifications_pending: false,
      },
    };
  };
  try {
    assert.deepEqual(mutations.setProfileLayout.mutationKey, [
      'setProfileLayout',
    ]);
    assert.equal(
      unwrapResult(await invokeMutation(mutations.setProfileLayout, [layout]))
        .status,
      'committed',
    );
    assert.deepEqual(calls, [layout]);
  } finally {
    commands.setProfileLayout = original;
  }
});

test('createProfileFolder forwards name and members and returns the actor-minted FolderId', async () => {
  const original = commands.createProfileFolder;
  const calls: [string, string[]][] = [];
  commands.createProfileFolder = async (name, members) => {
    calls.push([name, members]);
    return {
      status: 'ok',
      data: {
        status: 'committed',
        value: 'fminted',
        commits: [],
        notifications_pending: false,
      },
    };
  };
  try {
    assert.deepEqual(mutations.createProfileFolder.mutationKey, [
      'createProfileFolder',
    ]);
    const result = unwrapResult(
      await invokeMutation(mutations.createProfileFolder, ['New', ['a', 'b']]),
    );
    assert.equal(result.value, 'fminted');
    assert.deepEqual(calls, [['New', ['a', 'b']]]);
  } finally {
    commands.createProfileFolder = original;
  }
});

test('lost IPC reply is unconfirmed; does not claim the folder operation rolled back', async () => {
  const original = commands.setProfileLayout;
  commands.setProfileLayout = async () => {
    throw new Error('IPC lost after write');
  };
  try {
    await assert.rejects(
      invokeMutation(mutations.setProfileLayout, [layout]),
      (error: unknown) =>
        error instanceof MutationUnconfirmedError &&
        error.cause instanceof Error &&
        error.cause.message === 'IPC lost after write',
    );
  } finally {
    commands.setProfileLayout = original;
  }
});

test('remote import command forwards folder to the generated IPC as part of the request', async () => {
  const original = commands.importProfile;
  const calls: Array<Parameters<typeof commands.importProfile>> = [];
  commands.importProfile = async (...args) => {
    calls.push(args);
    return {
      status: 'ok',
      data: {
        status: 'committed',
        value: 'c-import',
        commits: [],
        notifications_pending: false,
      },
    };
  };
  try {
    const result = unwrapResult(
      await invokeMutation(mutations.importProfile, [
        'https://example.com/subscription',
        null,
        null,
        'f-work',
      ]),
    );
    assert.equal(result.value, 'c-import');
    assert.deepEqual(calls, [
      ['https://example.com/subscription', null, null, 'f-work'],
    ]);
  } finally {
    commands.importProfile = original;
  }
});

test('direct remote import retains the folder alongside the original mode', async () => {
  const original = commands.importProfileWithMode;
  const calls: Array<Parameters<typeof commands.importProfileWithMode>> = [];
  commands.importProfileWithMode = async (...args) => {
    calls.push(args);
    return {
      status: 'ok',
      data: {
        status: 'committed',
        value: 'c-direct',
        commits: [],
        notifications_pending: false,
      },
    };
  };
  try {
    const result = unwrapResult(
      await invokeMutation(mutations.importProfileWithMode, [
        'https://example.com/direct',
        null,
        null,
        'direct',
        'f-work',
      ]),
    );
    assert.equal(result.value, 'c-direct');
    assert.deepEqual(calls, [
      ['https://example.com/direct', null, null, 'direct', 'f-work'],
    ]);
  } finally {
    commands.importProfileWithMode = original;
  }
});

test('manual Config creation forwards its folder in the generated request', async () => {
  const original = commands.createProfile;
  const calls: Array<Parameters<typeof commands.createProfile>> = [];
  commands.createProfile = async (...args) => {
    calls.push(args);
    return {
      status: 'ok',
      data: {
        status: 'committed',
        value: 'c-local',
        commits: [],
        notifications_pending: false,
      },
    };
  };
  try {
    const request = {
      metadata: { name: 'Local', desc: null },
      definition: {
        type: 'config' as const,
        config: {
          type: 'file' as const,
          source: {
            type: 'local' as const,
            binding: { type: 'managed' as const, file: 'pending.yaml' },
          },
          transforms: [],
        },
      },
      folder: 'f-work',
    };
    const result = unwrapResult(
      await invokeMutation(mutations.createProfile, [request, 'proxies: []\n']),
    );
    assert.equal(result.value, 'c-local');
    assert.deepEqual(calls, [[request, 'proxies: []\n']]);
  } finally {
    commands.createProfile = original;
  }
});
