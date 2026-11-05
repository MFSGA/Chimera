/**
 * Isolated Node characterization of ref Glass segmented controls.
 * Mocked JSX and hooks do not establish real browser layout or focus behavior.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function flatten(classes) {
  return (Array.isArray(classes) ? classes.flat(Infinity) : [classes])
    .filter((value) => typeof value === 'string' && value.length > 0)
    .join(' ');
}

function cva(base, options = {}) {
  return (props = {}) => {
    const selections = Object.keys(options.variants || {}).map(
      (name) => options.variants[name][String(
        props[name] ?? options.defaultVariants?.[name],
      )],
    );
    return flatten([base, selections]);
  };
}

function fixture({ reducedMotion = false, rect = null, measured = true } = {}) {
  const effects = { layout: [], passive: [] };
  const changes = [];
  const calls = [];
  const refs = { root: { current: null }, items: { current: new Map() } };
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  let stateIndex = 0;
  let refIndex = 0;
  let currentContext = null;
  const context = { Provider: 'SegmentedContext.Provider', defaultValue: null };
  const mocks = {
    '@chimera/ui/ripple': {
      useRipple: () => ({
        ripples: [],
        onClick: () => calls.push('ripple'),
        onClear: () => {},
      }),
    },
    '@chimera/utils': { cn: (...classes) => flatten(classes) },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (next) => onChange?.(next),
      ],
    },
    '~icons/material-symbols/check-rounded': () => null,
    'class-variance-authority': { cva },
    'motion/react': {
      motion: { div: 'motion.div' },
      useReducedMotion: () => reducedMotion,
    },
    'radix-ui': {
      ToggleGroup: {
        Root: 'ToggleGroup.Root',
        Item: 'ToggleGroup.Item',
      },
    },
    react: {
      createContext: () => context,
      useContext: () => currentContext,
      useRef: () => [refs.root, refs.items][refIndex++],
      useState: (initial) => {
        const index = stateIndex++;
        const value = index === 0 ? rect : index === 1 ? measured : initial;
        return [
          value,
          (updater) => {
            const next = typeof updater === 'function' ? updater(value) : updater;
            changes.push({ index, next, same: next === value });
          },
        ];
      },
      useCallback: (callback) => callback,
      useMemo: (create) => create(),
      useEffect: (callback) => effects.passive.push(callback),
      useLayoutEffect: (callback) => effects.layout.push(callback),
      lazy: () => 'LazyRipple',
      Suspense: 'Suspense',
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const src = readFileSync(
    path.join(__dirname, '../src/segmented-button.tsx'),
    'utf8',
  );
  const compiled = ts.transpileModule(src, {
    fileName: 'segmented-button.tsx',
    reportDiagnostics: true,
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  });
  assert.deepEqual(
    (compiled.diagnostics || []).filter(
      (item) => item.category === ts.DiagnosticCategory.Error,
    ),
    [],
  );
  const module = { exports: {} };
  const run = new Function('require', 'module', 'exports', compiled.outputText);
  run(
    (name) => {
      assert.ok(Object.hasOwn(mocks, name), 'unexpected module: ' + name);
      return mocks[name];
    },
    module,
    module.exports,
  );
  return {
    ui: module.exports,
    effects,
    changes,
    calls,
    refs,
    setContext: (value) => {
      currentContext = value;
    },
  };
}

function find(node, predicate) {
  if (Array.isArray(node)) {
    for (const child of node) {
      const result = find(child, predicate);
      if (result) return result;
    }
  } else if (node && typeof node === 'object') {
    if (predicate(node)) return node;
    return find(node.props?.children, predicate);
  }
  return null;
}

function kind(tree, type) {
  const result = find(tree, (node) => node.type === type);
  assert.ok(result, 'missing ' + type);
  return result;
}

test('glass root and items add the reference variant without changing tabs', () => {
  const { ui } = fixture();
  const glassRoot = ui.segmentedButtonVariants({ variant: 'glass' });
  const glassItem = ui.segmentedButtonItemVariants({ variant: 'glass' });
  assert.match(glassRoot, /h-9 w-auto shrink-0 rounded-full/);
  assert.match(glassRoot, /bg-transparent/);
  assert.match(glassItem, /whitespace-nowrap/);
  assert.match(glassItem, /flex-none/);
  const tabs = ui.segmentedButtonVariants({ variant: 'tabs' });
  assert.match(tabs, /bg-surface-variant\/40/);
  assert.doesNotMatch(tabs, /w-auto shrink-0/);
});

test('glass selection shares tabs pill and respects reduced-motion', () => {
  for (const reducedMotion of [false, true]) {
    const env = fixture({
      rect: { x: 15, width: 66 },
      measured: true,
      reducedMotion,
    });
    const tree = env.ui.SegmentedButton({
      variant: 'glass',
      value: 'first',
      children: 'Segments',
    });
    const toggle = kind(tree, 'ToggleGroup.Root');
    assert.equal(toggle.props.value, 'first');
    const pill = kind(tree, 'motion.div');
    assert.equal(pill.props['data-slot'], 'segmented-button-slider-pill');
    assert.deepEqual(pill.props.animate, { x: 15, width: 66 });
    assert.deepEqual(
      pill.props.transition,
      reducedMotion
        ? { duration: 0 }
        : { type: 'spring', bounce: 0.15, duration: 0.45 },
    );
    const received = [];
    const changing = fixture();
    const changed = changing.ui.SegmentedButton({
      variant: 'glass',
      value: 'first',
      onValueChange: (next) => received.push(next),
    });
    kind(changed, 'ToggleGroup.Root').props.onValueChange('second');
    assert.deepEqual(received, ['second']);
  }
});

test('glass measures items, deduplicates identical rects and disconnects observer', () => {
  for (const variant of ['glass', 'tabs']) {
    const existing = { x: 40, width: 80 };
    const env = fixture({ rect: existing });
    const tree = env.ui.SegmentedButton({ variant, value: 'item' });
    const toggle = kind(tree, 'ToggleGroup.Root');
    toggle.props.ref.current = {
      getBoundingClientRect: () => ({ left: 10 }),
    };
    tree.props.value.registerItemRef('item', {
      getBoundingClientRect: () => ({ left: 50, width: 80 }),
    });
    env.effects.layout[0]();
    assert.equal(env.changes.at(-1)?.same, true);
    assert.equal(env.changes.at(-1)?.next, existing);

    const originalObserver = global.ResizeObserver;
    const observed = [];
    let disconnected = false;
    global.ResizeObserver = class {
      constructor(onResize) {
        this.onResize = onResize;
      }
      observe(node) {
        observed.push(node);
      }
      disconnect() {
        disconnected = true;
      }
    };
    try {
      const cleanup = env.effects.passive[0]();
      assert.equal(observed.length, 2);
      cleanup();
      assert.equal(disconnected, true);
    } finally {
      global.ResizeObserver = originalObserver;
    }
  }
  const filled = fixture();
  filled.ui.SegmentedButton({ variant: 'filled', value: 'item' });
  assert.equal(filled.effects.passive[0](), undefined);
});

test('glass items preserve state layer, click, disabled and hideIndicator API', () => {
  const env = fixture();
  const tree = env.ui.SegmentedButton({
    variant: 'glass',
    value: 'first',
  });
  env.setContext(tree.props.value);
  const clicked = [];
  const item = env.ui.SegmentedButtonItem({
    value: 'first',
    children: 'First',
    onClick: () => clicked.push('first'),
  });
  const toggle = kind(item, 'ToggleGroup.Item');
  assert.match(toggle.props.className, /whitespace-nowrap/);
  assert.ok(
    find(item, (node) =>
      node.props?.['data-slot'] === 'segmented-button-item-state-layer',
    ),
  );
  assert.equal(
    find(item, (node) => node.props?.['data-slot'] === 'check'),
    null,
  );
  toggle.props.onClick({});
  assert.deepEqual(clicked, ['first']);
  assert.deepEqual(env.calls, ['ripple']);
  const disabled = env.ui.SegmentedButtonItem({
    value: 'second',
    disabled: true,
    hideIndicator: true,
  });
  assert.equal(kind(disabled, 'ToggleGroup.Item').props.onClick, undefined);
});
