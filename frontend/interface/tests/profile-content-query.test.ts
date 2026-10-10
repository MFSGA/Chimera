import assert from 'node:assert/strict';
import test from 'node:test';
import { QueryClient } from '@tanstack/react-query';
import { commands } from '../src/ipc/bindings.js';
import { mutations, queries } from '../src/ipc/profile-bindings.js';
import {
  invokeMutation,
  MutationUnconfirmedError,
  unwrapQueryOptions,
} from '../src/ipc/query-options.js';
import { unwrapResult } from '../src/utils/index.js';

/**
 * Contract PROFILE-CONTENT-QUERY (unit): a Profile content read uses the
 * ref-style UID-scoped descriptor and unwraps the IPC result. A save delegates
 * the exact UID/content tuple and invalidates only that Profile's cache key.
 * Transport failures remain unconfirmed rather than masquerading as a
 * rejected Profile commit. No WebView, persistence, or core is exercised.
 */
test('content query forwards the selected UID and unwraps the result', async () => {
  const original = commands.readProfileFile;
  const calls: string[] = [];
  commands.readProfileFile = async (uid) => {
    calls.push(uid);
    return { status: 'ok', data: `raw:${uid}` };
  };

  try {
    const query = queries.readProfileFile('profile-a');
    assert.deepEqual(query.queryKey, ['readProfileFile', 'profile-a']);
    assert.deepEqual(queries.readProfileFile('profile-b').queryKey, [
      'readProfileFile',
      'profile-b',
    ]);
    assert.equal(
      await unwrapQueryOptions(query, query.queryFn!).queryFn(),
      'raw:profile-a',
    );
    assert.deepEqual(calls, ['profile-a']);
  } finally {
    commands.readProfileFile = original;
  }
});

test('content save delegates the tuple and scopes cache invalidation', async () => {
  const original = commands.saveProfileFile;
  const calls: [string, string][] = [];
  commands.saveProfileFile = async (uid, content) => {
    calls.push([uid, content]);
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
    const queryClient = new QueryClient();
    const own = queries.readProfileFile('profile-a').queryKey;
    const other = queries.readProfileFile('profile-b').queryKey;
    queryClient.setQueryData(own, 'old-a');
    queryClient.setQueryData(other, 'old-b');

    assert.deepEqual(mutations.saveProfileFile.mutationKey, [
      'saveProfileFile',
    ]);
    assert.equal(
      unwrapResult(
        await invokeMutation(mutations.saveProfileFile, [
          'profile-a',
          'updated content',
        ]),
      ).status,
      'committed',
    );
    await queryClient.invalidateQueries({ queryKey: own });

    assert.deepEqual(calls, [['profile-a', 'updated content']]);
    assert.equal(queryClient.getQueryState(own)?.isInvalidated, true);
    assert.equal(queryClient.getQueryState(other)?.isInvalidated, false);
  } finally {
    commands.saveProfileFile = original;
  }
});

test('a missing save reply stays unconfirmed instead of claiming rollback', async () => {
  const original = commands.saveProfileFile;
  commands.saveProfileFile = async () => {
    throw new Error('IPC disconnected after commit');
  };

  try {
    await assert.rejects(
      invokeMutation(mutations.saveProfileFile, ['profile-a', 'content']),
      (error: unknown) =>
        error instanceof MutationUnconfirmedError &&
        error.cause instanceof Error &&
        error.cause.message === 'IPC disconnected after commit',
    );
  } finally {
    commands.saveProfileFile = original;
  }
});
