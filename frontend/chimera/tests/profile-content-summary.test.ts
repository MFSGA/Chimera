import assert from 'node:assert/strict';
import test from 'node:test';
import { profileContentSummary } from '../src/pages/(main)/main/profiles/_modules/profile-content-summary';

test('counts only static YAML entries, excluding providers and group membership', () => {
  assert.deepEqual(
    profileContentSummary(`
proxies: [{ name: A }, { name: B }]
proxy-groups: [{ name: Auto, proxies: [A, B] }]
rules: [MATCH,DIRECT]
proxy-providers: { remote: { type: http } }
rule-providers: { remote: { type: http } }
`),
    { nodes: 2, groups: 1, rules: 2 },
  );
});

test('an empty mapping counts zero; explicit null values remain unknown', () => {
  assert.deepEqual(profileContentSummary('{}'), {
    nodes: 0,
    groups: 0,
    rules: 0,
  });
  // Match the pinned ref parser: a YAML Scalar(null) is not a missing field.
  assert.equal(
    profileContentSummary('proxies: null\nproxy-groups: []\nrules: []'),
    null,
  );
});

test('malformed, duplicate, non-mapping and unexpected field shapes remain unknown', () => {
  for (const content of [
    'proxies: [',
    'proxies: [a]\nproxies: [b]',
    'proxies: invalid',
    'rules: {not: a-sequence}',
    '- item',
    'null',
  ]) {
    assert.equal(profileContentSummary(content), null, content);
  }
});

test('YAML aliases pointing to a sequence are counted without following providers', () => {
  assert.deepEqual(
    profileContentSummary('nodes: &nodes [{name: A}]\nproxies: *nodes'),
    { nodes: 1, groups: 0, rules: 0 },
  );
});
