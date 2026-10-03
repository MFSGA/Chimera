import assert from 'node:assert/strict';
import test from 'node:test';
import { parseHotkey } from './parse-hotkey';

test('normalizes modifiers and named keyboard keys', () => {
  assert.equal(parseHotkey('Control'), 'CTRL');
  assert.equal(parseHotkey('Meta'), 'CMD');
  assert.equal(parseHotkey('ArrowLeft'), 'LEFT');
  assert.equal(parseHotkey(' '), 'SPACE');
});

test('uses the physical key code to distinguish keypad plus', () => {
  assert.equal(parseHotkey('+', 'Equal'), '+');
  assert.equal(parseHotkey('+', 'NumpadAdd'), 'NUMPADADD');
});
