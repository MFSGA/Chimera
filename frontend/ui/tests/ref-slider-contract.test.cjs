/**
 * A pure Node contract test for the real Slider source and event handlers.
 * TypeScript transpilation and boundary mocks do not simulate browser layout,
 * native pointer movement, or focus; use the upstream WebDriver tests for those.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function createSlider() {
  const source = readFileSync(path.join(__dirname, '../src/slider.tsx'), 'utf8');
  const compiled = ts.transpileModule(source, {
    fileName: 'slider.tsx',
    reportDiagnostics: true,
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
  });
  const errors = (compiled.diagnostics || []).filter(
    (d) => d.category === ts.DiagnosticCategory.Error,
  );
  assert.deepEqual(errors, [], 'slider.tsx should transpile');

  const jsx = (type, props, key) => ({ type, props, key });
  const mocks = {
    '@chimera/utils': {
      cn: (...classes) => classes.filter(Boolean).join(' '),
    },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (value) => onChange?.(value),
      ],
    },
    'motion/react': {
      clamp: (min, max, value) => Math.min(max, Math.max(min, value)),
    },
    react: {
      useRef: (value) => ({ current: value }),
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const fixture = { exports: {} };
  const requireMock = (name) => {
    assert.ok(Object.hasOwn(mocks, name), 'unexpected import: ' + name);
    return mocks[name];
  };
  const execute = new Function(
    'require',
    'module',
    'exports',
    compiled.outputText,
  );
  execute(requireMock, fixture, fixture.exports);
  assert.equal(typeof fixture.exports.Slider, 'function');
  return fixture.exports.Slider;
}

function child(tree, slot) {
  const node = tree.props.children.find(
    (part) => part.props?.['data-slot'] === slot,
  );
  assert.ok(node, 'missing ' + slot);
  return node;
}

function sliderInput(tree) {
  const node = tree.props.children.find((part) => part.type === 'input');
  assert.ok(node, 'native input is required');
  return node.props;
}

test('48px native thumb and painted handle share the reference geometry', () => {
  const Slider = createSlider();
  const tree = Slider({ value: 75, min: 0, max: 100, 'aria-label': 'Volume' });
  const input = sliderInput(tree);
  const thumb = child(tree, 'slider-thumb');
  const range = child(tree, 'slider-range');
  const track = child(tree, 'slider-track');

  assert.equal(input.type, 'range');
  assert.equal(input.value, 75);
  assert.equal(input['aria-label'], 'Volume');
  assert.equal(
    thumb.props.style.left,
    'calc(24px + (100% - 48px) * 0.75)',
  );
  assert.equal(
    range.props.style.width,
    'calc(calc(24px + (100% - 48px) * 0.75) - 8px)',
  );
  assert.equal(
    track.props.style.width,
    'calc(100% - calc(24px + (100% - 48px) * 0.75) - 8px)',
  );
  assert.match(input.className, /moz-range-thumb.*size-12/);
  assert.match(input.className, /webkit-slider-thumb.*size-12/);
  assert.match(tree.props.className, /h-12/);
});

test('commit uses most recent input even when controlled parent has not rendered', () => {
  const Slider = createSlider();
  const changes = [];
  const commits = [];
  const callbacks = [];
  const captured = [];
  const input = sliderInput(
    Slider({
      value: 50,
      onValueChange: (value) => changes.push(value),
      onValueCommit: (value) => commits.push(value),
      onPointerDown: () => callbacks.push('down'),
      onPointerUp: () => callbacks.push('up'),
      onPointerCancel: () => callbacks.push('cancel'),
    }),
  );

  input.onPointerDown({
    pointerId: 23,
    defaultPrevented: false,
    currentTarget: { setPointerCapture: (id) => captured.push(id) },
  });
  assert.deepEqual(captured, [23]);
  assert.deepEqual(callbacks, ['down']);

  input.onChange({ target: { value: '79' } });
  assert.deepEqual(changes, [79]);
  input.onPointerUp({ pointerId: 23 });
  assert.deepEqual(commits, [79]);
  assert.deepEqual(callbacks, ['down', 'up']);
  input.onBlur({});
  input.onPointerUp({ pointerId: 23 });
  assert.deepEqual(commits, [79], 'release and blur must not double-commit');

  input.onChange({ target: { value: '92' } });
  input.onPointerCancel({ pointerId: 23 });
  assert.deepEqual(commits, [79, 92]);
  assert.deepEqual(callbacks, ['down', 'up', 'up', 'cancel']);
});

test('keyboard commits once per changed value, never again on blur', () => {
  const Slider = createSlider();
  const commits = [];
  const input = sliderInput(
    Slider({ defaultValue: 50, onValueCommit: (v) => commits.push(v) }),
  );
  input.onKeyUp({ key: 'ArrowRight' });
  assert.deepEqual(commits, [], 'keyup without change is not a commit');
  input.onChange({ target: { value: '51' } });
  input.onKeyUp({ key: 'ArrowRight' });
  input.onBlur({});
  assert.deepEqual(commits, [51]);
  input.onChange({ target: { value: '100' } });
  input.onKeyUp({ key: 'End' });
  assert.deepEqual(commits, [51, 100]);
});

test('disabled and canceled pointer input preserve native constraints', () => {
  const Slider = createSlider();
  const tree = Slider({ defaultValue: 999, min: 1, max: 7, disabled: true });
  const input = sliderInput(tree);
  assert.equal(input.disabled, true);
  assert.equal(input.value, 7);
  assert.equal(tree.props['data-disabled'], 'true');

  let captured = 0;
  input.onPointerDown({
    pointerId: 4,
    defaultPrevented: false,
    currentTarget: { setPointerCapture: () => captured++ },
  });
  assert.equal(captured, 0, 'disabled input does not capture a pointer');

  const active = sliderInput(
    Slider({
      defaultValue: 5,
      onPointerDown: (event) => {
        event.defaultPrevented = true;
      },
    }),
  );
  active.onPointerDown({
    pointerId: 4,
    defaultPrevented: false,
    currentTarget: { setPointerCapture: () => captured++ },
  });
  assert.equal(captured, 0, 'consumer preventDefault cancels capture');
});
