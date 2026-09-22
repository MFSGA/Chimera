import assert from 'node:assert/strict';
import test from 'node:test';
import {
  assertServiceReleaseCommit,
  assertStableServiceRelease,
  EXPECTED_SERVICE_ASSETS,
  type ServiceReleaseMetadata,
} from './service-release.ts';

const stableRelease = (): ServiceReleaseMetadata => ({
  tag_name: 'v1.10.0',
  draft: false,
  prerelease: false,
  assets: EXPECTED_SERVICE_ASSETS.map((name) => ({ name })),
});

test('accepts a stable release with every required asset', () => {
  assert.doesNotThrow(() =>
    assertStableServiceRelease(stableRelease(), 'v1.10.0'),
  );
});

test('rejects draft and prerelease candidates', () => {
  assert.throws(
    () =>
      assertStableServiceRelease(
        { ...stableRelease(), draft: true },
        'v1.10.0',
      ),
    /still a draft/,
  );
  assert.throws(
    () =>
      assertStableServiceRelease(
        { ...stableRelease(), prerelease: true },
        'v1.10.0',
      ),
    /prerelease candidate/,
  );
});

test('accepts only the exact pinned service commit', () => {
  const commit = '0123456789abcdef0123456789abcdef01234567';
  assert.doesNotThrow(() =>
    assertServiceReleaseCommit(commit, commit.toUpperCase()),
  );
  assert.throws(
    () =>
      assertServiceReleaseCommit(
        commit,
        'fedcba9876543210fedcba9876543210fedcba98',
      ),
    /release commit mismatch/,
  );
  assert.throws(
    () => assertServiceReleaseCommit('not-a-sha', commit),
    /invalid pinned/,
  );
});

test('rejects the wrong tag or incomplete assets', () => {
  assert.throws(
    () => assertStableServiceRelease(stableRelease(), 'v1.10.1'),
    /tag mismatch/,
  );

  const release = stableRelease();
  release.assets = release.assets.slice(1);
  assert.throws(
    () => assertStableServiceRelease(release, 'v1.10.0'),
    /missing assets/,
  );
});
