import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  LogLevel,
  matchesLogLevel,
} from '../src/pages/(main)/main/logs/_modules/consts';

test('the historical WS warning level is reachable through the ref warning filter', () => {
  assert.equal(matchesLogLevel('warn', LogLevel.Warning), true);
  assert.equal(matchesLogLevel('warning', LogLevel.Warning), true);
  assert.equal(matchesLogLevel('WARN', LogLevel.Warning), true);
  assert.equal(matchesLogLevel('error', LogLevel.Warning), false);
});

test('other levels match exactly and All does not discard existing log rows', () => {
  for (const level of Object.values(LogLevel)) {
    assert.equal(matchesLogLevel(level.toUpperCase(), level), true);
  }
  assert.equal(matchesLogLevel('info', LogLevel.Fatal), false);
  assert.equal(matchesLogLevel('trace', null), true);
  assert.equal(matchesLogLevel('custom', undefined), true);
});
