import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  fingerprint,
  firstFrame,
  normalizeMessage,
} from '../src/services/error-reporting/fingerprint';
import {
  describeError,
  fromConsole,
  fromReact,
  fromRejection,
  serializeValue,
  stripQuery,
} from '../src/services/error-reporting/normalize';

test('serializes values without throwing on cycles, depth or size', () => {
  const cyclic: Record<string, unknown> = { name: 'node' };
  cyclic.self = cyclic;
  assert.equal(serializeValue(cyclic), '{"name":"node","self":"[Circular]"}');

  assert.equal(
    serializeValue({ a: { b: { c: { d: { e: 1 } } } } }),
    '{"a":{"b":{"c":{"d":"[Object]"}}}}',
  );
  assert.equal(serializeValue(10n), '10n');
  assert.equal(serializeValue(undefined), '[undefined]');
  assert.equal(
    serializeValue(function load() {}),
    '[Function load]',
  );
  assert.equal(serializeValue(new TypeError('bad')), 'TypeError: bad');
  assert.equal(serializeValue('x'.repeat(5000)).length, 4096);

  const hostile = {
    get value() {
      throw new Error('getter');
    },
  };
  assert.equal(serializeValue(hostile), '[object Object]');
});

test('describes an error with its bounded cause chain', () => {
  const root = new Error('root');
  const middle = new Error('middle', { cause: root });
  const error = new RangeError('outer', { cause: middle });

  const described = describeError(error);

  assert.equal(described.message, 'outer');
  assert.equal(described.error_name, 'RangeError');
  assert.ok(described.stack?.includes('outer'));
  assert.deepEqual(
    described.causes.map((cause) => cause.message),
    ['middle', 'root'],
  );

  const looped = new Error('a');
  looped.cause = looped;
  assert.deepEqual(describeError(looped).causes, []);

  const deep = Array.from({ length: 6 }).reduce<Error>(
    (cause, _, i) => new Error(`level ${i}`, { cause }),
    new Error('bottom'),
  );
  assert.equal(describeError(deep).causes.length, 3);

  assert.deepEqual(describeError({ code: 1 }), {
    message: '{"code":1}',
    error_name: null,
    stack: null,
    causes: [],
  });
});

test('console arguments become one message and keep the first error stack', () => {
  const error = new Error('broken');
  const draft = fromConsole('error', ['failed to load', error, { id: 1 }]);

  assert.partialDeepStrictEqual(draft, {
    kind: 'console',
    level: 'error',
    message: 'failed to load Error: broken {"id":1}',
    error_name: 'Error',
    stack: error.stack,
  });
  assert.equal(fromConsole('warning', ['plain']).stack, null);
});

test('rejections and React errors map to their kinds and levels', () => {
  assert.partialDeepStrictEqual(fromRejection('nope'), {
    kind: 'unhandled_rejection',
    level: 'error',
    message: 'nope',
  });
  assert.partialDeepStrictEqual(
    fromReact('react_caught', new Error('x'), '\n    at Page'),
    {
      kind: 'react_caught',
      level: 'error',
      component_stack: '\n    at Page',
    },
  );
  assert.partialDeepStrictEqual(
    fromReact('react_recoverable', new Error('x'), undefined),
    { level: 'warning', component_stack: null },
  );
});

test('query strings are dropped from URLs and frames', () => {
  assert.equal(
    stripQuery('https://host/sub?token=secret#frag'),
    'https://host/sub',
  );
  assert.equal(
    firstFrame(
      'Error: x\n    at load (http://localhost/assets/app.js?v=3:10:5)\n    at run',
    ),
    'at load (http://localhost/assets/app.js:10:5)',
  );
  assert.equal(
    firstFrame('load@http://localhost/app.js:1:2\nrun@x'),
    'load@http://localhost/app.js:1:2',
  );
  assert.equal(firstFrame(null), '');
});

test('fingerprints group repeats that differ only in variable values', () => {
  const draft = (message: string) =>
    fromConsole('error', [message, new Error('same')]);

  assert.equal(
    normalizeMessage(
      'id 42 at 7f3a9c0b1d2e uuid 0b0f1c3e-1111-4a2b-8c3d-1234567890ab',
    ),
    'id <n> at <hex> uuid <uuid>',
  );
  assert.equal(
    fingerprint(draft('retry 1 of 3')),
    fingerprint(draft('retry 2 of 3')),
  );
  assert.notEqual(
    fingerprint(draft('retry 1 of 3')),
    fingerprint(draft('timeout')),
  );
  assert.match(fingerprint(draft('x')), /^console-[0-9a-f]+$/);
  assert.match(fingerprint(fromRejection('x')), /^unhandled_rejection-/);
});
