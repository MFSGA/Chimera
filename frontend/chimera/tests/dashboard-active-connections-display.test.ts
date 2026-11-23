import assert from 'node:assert/strict';
import test from 'node:test';
import { connectionWidgetStreamState } from '../src/features/dashboard/active-connections-display.ts';

test('controller and connection-details stream each report their own connecting stage', () => {
  assert.equal(connectionWidgetStreamState('connecting', 'idle'), 'connecting');
  assert.equal(
    connectionWidgetStreamState('connected', 'connecting'),
    'connecting',
  );
  assert.equal(connectionWidgetStreamState('connected', 'idle'), 'ready');
});

test('disconnected controller does not show stale detail stream or errors', () => {
  assert.equal(
    connectionWidgetStreamState('disconnected', 'connected'),
    'disconnected',
  );
  assert.equal(
    connectionWidgetStreamState('disconnected', 'error'),
    'disconnected',
  );
  assert.equal(
    connectionWidgetStreamState('connecting', 'error'),
    'connecting',
  );
});

test('a connected controller surfaces stream failure without hiding the retry action', () => {
  assert.equal(connectionWidgetStreamState('connected', 'error'), 'error');
  assert.equal(connectionWidgetStreamState('connected', 'connected'), 'ready');
});
