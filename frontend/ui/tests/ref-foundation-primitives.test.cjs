/**
 * Node contracts for pinned-ref shared UI primitives. This executes the real
 * production TSX through TypeScript's compiler while mocking JSX, React, and
 * Motion boundaries; it does not replace browser/desktop interaction tests.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function fixture({ reducedMotion = false, imageLoading = true } = {}) {
  const stateChanges = [];
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const mocks = {
    '@chimera/utils': {
      cn: (...values) => values.filter(Boolean).join(' '),
    },
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { div: 'motion.div', span: 'motion.span' },
      useReducedMotion: () => reducedMotion,
    },
    react: {
      useState: () => [imageLoading, (next) => stateChanges.push(next)],
    },
    'react/jsx-runtime': { jsx, jsxs: jsx, Fragment: 'Fragment' },
  };
  const load = (name) => {
    const filePath = path.join(__dirname, '../src', name + '.tsx');
    const source = readFileSync(filePath, 'utf8');
    const compiled = ts.transpileModule(source, {
      fileName: filePath,
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
        (diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error,
      ),
      [],
      name + ' must transpile',
    );
    const module = { exports: {} };
    const execute = new Function(
      'require',
      'module',
      'exports',
      compiled.outputText,
    );
    execute(
      (specifier) => {
        assert.ok(Object.hasOwn(mocks, specifier), 'unmocked ' + specifier);
        return mocks[specifier];
      },
      module,
      module.exports,
    );
    return module.exports;
  };
  return { load, stateChanges };
}

function find(tree, predicate) {
  if (Array.isArray(tree)) {
    for (const element of tree) {
      const result = find(element, predicate);
      if (result) return result;
    }
    return null;
  }
  if (!tree || typeof tree !== 'object') return null;
  if (predicate(tree)) return tree;
  return find(tree.props?.children, predicate);
}

test('Kbd forwards DOM props and composes the pinned ref styles', () => {
  const { Kbd } = fixture().load('kbd');
  const node = Kbd({
    className: 'custom-class',
    children: 'Ctrl',
    title: 'Keyboard shortcut',
    'aria-label': 'Control key',
  });
  assert.equal(node.type, 'kbd');
  assert.equal(node.props.title, 'Keyboard shortcut');
  assert.equal(node.props['aria-label'], 'Control key');
  assert.equal(node.props.children, 'Ctrl');
  assert.match(node.props.className, /font-mono/);
  assert.match(node.props.className, /rounded-3xl/);
  assert.match(node.props.className, /custom-class/);
});

test('ActionSwap swaps by stable contentKey with overlapping exit animation', () => {
  const { ActionSwap } = fixture().load('action-swap-text');
  const node = ActionSwap({
    contentKey: 'page-two',
    children: 'New page',
    className: 'custom',
    'aria-label': 'Animated region',
  });
  assert.equal(node.type, 'div');
  assert.equal(node.props['data-slot'], 'action-swap');
  assert.equal(node.props['aria-label'], 'Animated region');
  assert.match(node.props.className, /grid overflow-hidden custom/);
  const presence = find(node, (child) => child.type === 'AnimatePresence');
  assert.equal(presence.props.mode, 'popLayout');
  assert.equal(presence.props.initial, false);
  const animated = find(node, (child) => child.type === 'motion.div');
  assert.equal(animated.key, 'page-two');
  assert.equal(animated.props.children, 'New page');
  assert.deepEqual(animated.props.initial, {
    opacity: 0,
    filter: 'blur(4px)',
    y: 8,
  });
  assert.deepEqual(animated.props.exit, {
    opacity: 0,
    filter: 'blur(4px)',
    y: -8,
  });
  assert.equal(animated.props.transition.duration, 0.18);
  const hidden = ActionSwap({ contentKey: null, children: 'Not mounted' });
  assert.equal(find(hidden, (child) => child.type === 'motion.div'), null);
});

test('ActionSwapText keys content and respects reduced motion', () => {
  const { ActionSwap, ActionSwapText } = fixture({ reducedMotion: true }).load(
    'action-swap-text',
  );
  const wrapper = ActionSwapText({
    value: 'Connected',
    className: 'text-sm',
    id: 'status',
  });
  assert.equal(wrapper.type, ActionSwap);
  assert.equal(wrapper.props.contentKey, 'Connected');
  assert.equal(wrapper.props.children, 'Connected');
  assert.equal(wrapper.props['data-slot'], 'action-swap-text');
  assert.equal(wrapper.props.id, 'status');
  const animated = find(
    ActionSwap(wrapper.props),
    (node) => node.type === 'motion.div',
  );
  assert.equal(animated.props.initial.filter, 'blur(0px)');
  assert.equal(animated.props.initial.y, 0);
  assert.equal(animated.props.exit.y, 0);
  assert.deepEqual(animated.props.transition, { duration: 0 });
});

test('MaterialShape honors active, square and depth variants with 96 vertices', () => {
  const { MaterialShape } = fixture().load('material-shape');
  const active = MaterialShape({ className: 'custom-shape', title: 'Shape' });
  const soft = MaterialShape({ active: false });
  const square = MaterialShape({ active: false, inactiveSquare: true });
  const deep = MaterialShape({ depth: 12, active: false, inactiveSquare: true });
  assert.equal(active.type, 'motion.span');
  assert.equal(active.props.title, 'Shape');
  assert.equal(active.props['data-slot'], 'material-shape');
  assert.equal(active.props.initial, false);
  assert.match(active.props.className, /custom-shape/);
  for (const node of [active, soft, square, deep]) {
    assert.equal(node.props.animate.clipPath.startsWith('polygon('), true);
    assert.equal(node.props.animate.clipPath.split('%').length - 1, 192);
    assert.equal(node.props.transition.duration, 0.4);
  }
  assert.notEqual(active.props.animate.clipPath, soft.props.animate.clipPath);
  assert.notEqual(soft.props.animate.clipPath, square.props.animate.clipPath);
  assert.notEqual(deep.props.animate.clipPath, square.props.animate.clipPath);
});

test('MaterialShape disables animation when OS reduced motion is requested', () => {
  const { MaterialShape } = fixture({ reducedMotion: true }).load(
    'material-shape',
  );
  const node = MaterialShape({ active: true });
  assert.deepEqual(node.props.transition, {
    duration: 0,
    ease: [0.22, 1, 0.36, 1],
  });
});

test('LazyImage preserves loader and forwards load events and image attrs', () => {
  const ui = fixture();
  const { LazyImage } = ui.load('lazy-image');
  const loaded = [];
  const node = LazyImage({
    src: '/icon.png',
    alt: 'Icon',
    className: 'icon',
    loadingClassName: 'busy',
    onLoad: (event) => loaded.push(event.type),
  });
  assert.equal(node.type, 'Fragment');
  const loader = find(node, (child) => child.type === 'div');
  const image = find(node, (child) => child.type === 'img');
  assert.match(loader.props.className, /animate-pulse/);
  assert.match(loader.props.className, /busy/);
  assert.match(image.props.className, /hidden/);
  assert.equal(image.props.src, '/icon.png');
  assert.equal(image.props.alt, 'Icon');
  image.props.onLoad({ type: 'load' });
  assert.deepEqual(ui.stateChanges, [false]);
  assert.deepEqual(loaded, ['load']);
  const settled = fixture({ imageLoading: false }).load('lazy-image').LazyImage({
    src: '/icon.png',
  });
  assert.match(
    find(settled, (child) => child.type === 'div').props.className,
    /hidden/,
  );
  assert.match(
    find(settled, (child) => child.type === 'img').props.className,
    /inline-block/,
  );
});
