import assert from 'node:assert/strict';
import test from 'node:test';
import {
  isPrivacySafeSnapshot,
  serializePrivacySafeSnapshot,
  type PrivacySafeSnapshot,
} from '../frontend/chimera/src/features/agent/model/privacy-safe-context.ts';

const safePrivacy = {
  contains_raw_logs: false,
  contains_profile_names: false,
  contains_profile_urls: false,
  contains_connection_targets: false,
  contains_controller_secret: false,
} satisfies PrivacySafeSnapshot['privacy'];

const safeSnapshot = {
  revision: 'privacy-test',
  privacy: safePrivacy,
};

test('agent diagnostic context serializes only when every privacy assertion is false', () => {
  assert.equal(isPrivacySafeSnapshot(safeSnapshot), true);
  assert.match(
    serializePrivacySafeSnapshot(safeSnapshot) ?? '',
    /privacy-test/,
  );
});

test('agent diagnostic context fails closed for every privacy boundary', () => {
  for (const key of Object.keys(safeSnapshot.privacy) as Array<
    keyof typeof safeSnapshot.privacy
  >) {
    const unsafeSnapshot = {
      ...safeSnapshot,
      privacy: { ...safeSnapshot.privacy, [key]: true },
    };

    assert.equal(isPrivacySafeSnapshot(unsafeSnapshot), false, String(key));
    assert.equal(
      serializePrivacySafeSnapshot(unsafeSnapshot),
      null,
      String(key),
    );
  }
});
