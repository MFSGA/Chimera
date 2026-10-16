import assert from 'node:assert/strict';
import test from 'node:test';
import { KvStorageWriteEchoes } from '../src/hooks/kv-storage-write-echoes.ts';

test('retains and consumes a delayed write echo after switching A to B to A', () => {
  const echoes = new KvStorageWriteEchoes('A');
  echoes.add('A', 'old-value');

  echoes.setActiveKey('B');
  echoes.setActiveKey('A');
  echoes.add('A', 'new-value');

  assert.equal(echoes.consume('A', 'old-value'), true);
  assert.equal(echoes.consume('A', 'new-value'), true);
  assert.equal(echoes.consume('A', 'old-value'), false);
});

test('consumes a completed write echo while another key is active', () => {
  const echoes = new KvStorageWriteEchoes('A');
  echoes.add('A', 'value');

  echoes.setActiveKey('B');

  assert.equal(echoes.consume('A', 'value'), true);
  assert.equal(echoes.isActiveKey('B'), true);
  assert.equal(echoes.consume('A', 'value'), false);
});

test('keeps duplicate same-value writes pending until each echo arrives', () => {
  const echoes = new KvStorageWriteEchoes('A');
  echoes.add('A', 'same-value');
  echoes.add('A', 'same-value');

  assert.equal(echoes.consume('A', 'same-value'), true);
  assert.equal(echoes.consume('A', 'same-value'), true);
  assert.equal(echoes.consume('A', 'same-value'), false);
});
