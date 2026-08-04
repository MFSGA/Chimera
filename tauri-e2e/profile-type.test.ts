import assert from 'node:assert/strict';
import test from 'node:test';
import {
  parseProfileType,
  ProfileType,
} from '../frontend/chimera/src/pages/(main)/main/profiles/_modules/profile-type.ts';

test('profile type parser accepts every supported route value', () => {
  for (const type of Object.values(ProfileType)) {
    assert.equal(parseProfileType(type), type);
  }
});

test('profile type parser rejects uppercase and mixed-case variants', () => {
  for (const type of ['PROFILE', 'Profile', 'JavaScript', 'MERGE']) {
    assert.equal(parseProfileType(type), null);
  }
});

test('profile type parser rejects empty and whitespace-padded values', () => {
  for (const type of ['', ' ', ' profile', 'profile ', '\tprofile']) {
    assert.equal(parseProfileType(type), null);
  }
});

test('profile type parser rejects path-like and encoded values', () => {
  for (const type of [
    '../profile',
    'profile/detail',
    'profile%2Fdetail',
    '%2E%2E',
    'profile?tab=detail',
    'profile#detail',
  ]) {
    assert.equal(parseProfileType(type), null);
  }
});

test('profile type parser rejects unsupported future-looking values', () => {
  for (const type of ['remote', 'local', 'script', 'yaml', 'json']) {
    assert.equal(parseProfileType(type), null);
  }
});

test('profile type parser does not coerce non-canonical Unicode', () => {
  for (const type of ['ｐｒｏｆｉｌｅ', 'profıle', 'profile\u0000']) {
    assert.equal(parseProfileType(type), null);
  }
});
