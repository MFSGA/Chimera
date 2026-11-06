/**
 * Node contract tests for the real Modal JSX using a mocked React/Radix/Motion
 * boundary. These do not measure layout or replace browser/desktop E2E.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
async function loadModal() {
  // TypeScript 7 no longer exposes transpileModule. Vite is already part of
  // this workspace, so transform the real component through its TSX pipeline.
  const { transformWithEsbuild } = await import('vite');
  const source = readFileSync(path.join(__dirname, '../src/modal.tsx'), 'utf8');
  const compiled = await transformWithEsbuild(source, 'modal.tsx', {
    loader: 'tsx',
    jsx: 'automatic',
    format: 'cjs',
    target: 'es2022',
  });

  let currentContext = null;
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const dialog = Object.fromEntries(
    [
      'Portal',
      'Title',
      'Description',
      'Trigger',
      'Close',
      'Overlay',
      'Content',
      'Root',
    ].map((name) => [name, 'Dialog.' + name]),
  );
  const mocks = {
    '@chimera/utils': {
      cn: (...classes) => classes.filter(Boolean).join(' '),
    },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (next) => onChange?.(next),
      ],
    },
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { div: 'motion.div' },
    },
    'radix-ui': {
      Dialog: dialog,
      Slot: { Root: 'Slot.Root', Slottable: 'Slot.Slottable' },
    },
    react: {
      createContext: (defaultValue) => ({
        Provider: 'ModalContext.Provider',
        defaultValue,
      }),
      useContext: (context) => currentContext ?? context.defaultValue,
      useId: () => 'modal-layout-test-id',
    },
    './button': { Button: 'Button' },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const module = { exports: {} };
  const evaluate = new Function('require', 'module', 'exports', compiled.code);
  evaluate(
    (name) => {
      assert.ok(Object.hasOwn(mocks, name), 'unmocked dependency: ' + name);
      return mocks[name];
    },
    module,
    module.exports,
  );

  return {
    ...module.exports,
    setContext: (value) => {
      currentContext = value;
    },
  };
}

function walk(tree, predicate) {
  if (Array.isArray(tree)) {
    for (const item of tree) {
      const found = walk(item, predicate);
      if (found) return found;
    }
    return null;
  }
  if (!tree || typeof tree !== 'object') return null;
  if (predicate(tree)) return tree;
  return walk(tree.props?.children, predicate);
}

function slot(tree, name) {
  const found = walk(tree, (node) => node.props?.['data-slot'] === name);
  assert.ok(found, 'missing data-slot ' + name);
  return found;
}

function kind(tree, name) {
  const found = walk(tree, (node) => node.type === name);
  assert.ok(found, 'missing node ' + name);
  return found;
}

test('controlled Modal forwards changes and honors defaultOpen', async () => {
  const ui = await loadModal();
  const changes = [];
  const controlled = ui.Modal({
    open: true,
    onOpenChange: (open) => changes.push(open),
    children: 'content',
  });
  assert.equal(controlled.props.value.open, true);
  assert.equal(controlled.props.value.layoutId, 'modal-layout-test-id');
  const root = kind(controlled, 'Dialog.Root');
  assert.equal(root.props.open, true);
  assert.equal(root.props.children, 'content');
  root.props.onOpenChange(false);
  assert.deepEqual(changes, [false]);

  const initial = ui.Modal({ defaultOpen: false });
  assert.equal(initial.props.value.open, false);
  const defaultOpen = ui.Modal({ defaultOpen: true });
  assert.equal(defaultOpen.props.value.open, true);
});

test('trigger layout measures only when open state changes', async () => {
  const ui = await loadModal();
  for (const open of [false, true]) {
    const owner = ui.Modal({ open });
    ui.setContext(owner.props.value);
    const trigger = ui.ModalTrigger({ asChild: true, children: 'Launch' });
    const primitive = slot(trigger, 'modal-trigger');
    const placeholder = slot(trigger, 'modal-trigger-placeholder');
    assert.equal(primitive.props['data-layout-id'], owner.props.value.layoutId);
    assert.equal(placeholder.props.layout, true);
    assert.equal(placeholder.props.layoutId, owner.props.value.layoutId);
    assert.equal(placeholder.props.layoutDependency, open);
    assert.equal(kind(trigger, 'Slot.Root').type, 'Slot.Root');
    const repeat = slot(
      ui.ModalTrigger({ asChild: true, children: 'Launch again' }),
      'modal-trigger-placeholder',
    );
    assert.equal(repeat.props.layoutDependency, open);
  }
});

test('open content preserves layout dependency across rerenders', async () => {
  const ui = await loadModal();
  const owner = ui.Modal({ open: true });
  ui.setContext(owner.props.value);
  const first = ui.ModalContent({ children: 'First render', id: 'dialog' });
  const second = ui.ModalContent({ children: 'Second render', id: 'dialog' });
  for (const tree of [first, second]) {
    const presence = kind(tree, 'AnimatePresence');
    assert.equal(presence.props.initial, false);
    assert.equal(kind(tree, 'Dialog.Portal').props.forceMount, true);
    const content = slot(tree, 'modal-content');
    assert.equal(content.props.id, 'dialog');
    assert.equal(content.props['data-layout-id'], owner.props.value.layoutId);
    const animated = kind(tree, 'motion.div');
    assert.equal(animated.props.layout, true);
    assert.equal(animated.props.layoutId, owner.props.value.layoutId);
    assert.equal(animated.props.layoutDependency, 'modal-content');
  }
  const closed = ui.Modal({ open: false });
  ui.setContext(closed.props.value);
  assert.equal(
    kind(ui.ModalContent({ children: 'Hidden' }), 'AnimatePresence').props
      .children,
    false,
  );
});

test('existing context guard and ModalClose asChild API remain intact', async () => {
  const ui = await loadModal();
  assert.throws(() => ui.ModalTrigger({ children: 'Orphan' }), /inside Modal/);
  assert.throws(() => ui.ModalContent({ children: 'Orphan' }), /inside Modal/);
  const direct = ui.ModalClose({ asChild: true, children: 'Custom control' });
  assert.equal(direct.type, 'Dialog.Close');
  assert.equal(direct.props.asChild, true);
  assert.equal(direct.props.children, 'Custom control');
  const automatic = ui.ModalClose({ children: 'Dismiss' });
  assert.equal(kind(automatic, 'Button').props.children, 'Dismiss');
});
