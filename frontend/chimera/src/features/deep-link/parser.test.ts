import assert from 'node:assert/strict';
import test from 'node:test';
import { parseDeepLink } from './parser';

test('parses public deep-link commands', () => {
  assert.deepEqual(parseDeepLink('chimera://system-proxy?mode=on'), {
    type: 'system-proxy',
    mode: 'on',
  });
  assert.deepEqual(parseDeepLink('chimera://system-proxy?mode=off'), {
    type: 'system-proxy',
    mode: 'off',
  });
  assert.deepEqual(parseDeepLink('chimera://system-proxy?mode=toggle'), {
    type: 'system-proxy',
    mode: 'toggle',
  });
  assert.deepEqual(parseDeepLink('chimera://language?locale=zh-cn'), {
    type: 'language',
    locale: 'zh-cn',
  });
});

test('rejects invalid proxy modes and unknown locales', () => {
  assert.throws(
    () => parseDeepLink('chimera://system-proxy?mode=abc'),
    /mode must be on, off, or toggle/,
  );
  assert.throws(
    () => parseDeepLink('chimera://language?locale=whatever'),
    /unsupported locale/,
  );
  assert.throws(
    () => parseDeepLink('chimera://language?locale=ZH-CN'),
    /unsupported locale/,
  );
});

test('validates install-config URLs before execution', () => {
  assert.deepEqual(
    parseDeepLink(
      'chimera://install-config?url=https%3A%2F%2Fexample.com%2Fsub.yaml&name=Demo',
    ),
    {
      type: 'install-config',
      url: 'https://example.com/sub.yaml',
      name: 'Demo',
    },
  );
  assert.throws(
    () => parseDeepLink('chimera://install-config?url=file%3A%2F%2F%2Ftmp%2Fa'),
    /must use http or https/,
  );
});
