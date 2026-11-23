import assert from 'node:assert/strict';
import test from 'node:test';
import {
  canToggleProviderReference,
  providerReferenceChoices,
} from '../src/components/widgets/widget-reference-model.ts';

const selected = [
  { kind: 'proxy' as const, name: 'old-provider' },
  { kind: 'rule' as const, name: 'custom-rules' },
];

test('pending lists never claim a persisted reference has been deleted', () => {
  const choices = providerReferenceChoices(selected, null, null);
  assert.deepEqual(
    choices.map(({ availability }) => availability),
    ['unknown', 'unknown'],
  );
  assert.deepEqual(
    choices.map(({ ref }) => ref),
    selected,
  );
  assert.equal(canToggleProviderReference(choices[0]!, true), true);
  assert.equal(canToggleProviderReference(choices[1]!, false), false);
});

test('successfully fetched empty provider list marks only that kind as missing', () => {
  const choices = providerReferenceChoices(selected, [], null);
  assert.deepEqual(
    choices.map(({ availability }) => availability),
    ['missing', 'unknown'],
  );
  assert.equal(canToggleProviderReference(choices[0]!, true), true);
  assert.equal(canToggleProviderReference(choices[0]!, false), false);
});

test('recovered queries retain saved refs and append each newly available provider once', () => {
  const choices = providerReferenceChoices(
    selected,
    ['old-provider', 'new-provider'],
    ['new-rule', 'custom-rules'],
  );
  assert.deepEqual(
    choices.map(({ ref }) => `${ref.kind}:${ref.name}`),
    [
      'proxy:old-provider',
      'rule:custom-rules',
      'proxy:new-provider',
      'rule:new-rule',
    ],
  );
  assert.deepEqual(
    choices.map(({ availability }) => availability),
    ['available', 'available', 'available', 'available'],
  );
  assert.equal(canToggleProviderReference(choices[2]!, false), true);
});

test('same-name proxy and rule providers are kept distinct', () => {
  const choices = providerReferenceChoices(
    [{ kind: 'proxy', name: 'shared' }],
    ['shared'],
    ['shared'],
  );
  assert.deepEqual(
    choices.map(({ ref }) => ref),
    [
      { kind: 'proxy', name: 'shared' },
      { kind: 'rule', name: 'shared' },
    ],
  );
});

test('a failed provider kind does not hide names from a successful kind', () => {
  const choices = providerReferenceChoices(selected, ['fresh'], null);
  assert.deepEqual(
    choices.map(({ availability }) => availability),
    ['missing', 'unknown', 'available'],
  );
  assert.deepEqual(choices[2]!.ref, { kind: 'proxy', name: 'fresh' });
});
