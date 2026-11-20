import assert from 'node:assert/strict';
import test from 'node:test';
import { maskAddress } from '../src/components/widgets/network-info-utils.ts';

test('masking hides IPv4 digits without modifying separators', () => {
  assert.equal(maskAddress('203.0.113.7'), '•••.•.•••.•');
});

test('masking hides IPv6 hex letters and digits but leaves its shape', () => {
  assert.equal(maskAddress('2001:db8::abc:1'), '••••:•••::•••:•');
});

test('address masking does not alter the stored source string', () => {
  const address = '2001:db8::1';
  assert.notEqual(maskAddress(address), address);
  assert.equal(address, '2001:db8::1');
});
