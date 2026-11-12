/**
 * Node contract tests for the real Modal JSX using a mocked React/Radix/Motion
 * boundary. These do not measure layout or replace browser/desktop E2E.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
global.Element ??= class Element {};

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
  let nextId = 0;
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
    ].map((name) => [name, `Dialog.${name}`]),
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
      isValidElement: (node) =>
        Boolean(node && typeof node === 'object' && 'props' in node),
      useCallback: (callback) => callback,
      useContext: (context) => currentContext ?? context.defaultValue,
      useEffect: (effect) => effect(),
      useId: () => `modal-trigger-${++nextId}`,
      useRef: (current) => ({ current }),
      useState: (initial) => {
        let current = initial;
        return [
          current,
          (next) => {
            current = typeof next === 'function' ? next(current) : next;
          },
        ];
      },
    },
    './button': { Button: 'Button' },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const module = { exports: {} };
  const evaluate = new Function('require', 'module', 'exports', compiled.code);
  evaluate(
    (name) => {
      assert.ok(Object.hasOwn(mocks, name), `unmocked dependency: ${name}`);
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
  assert.ok(found, `missing data-slot ${name}`);
  return found;
}

function kind(tree, name) {
  const found = walk(tree, (node) => node.type === name);
  assert.ok(found, `missing node ${name}`);
  return found;
}

function renderPlaceholder(tree, ui) {
  const placeholder = walk(
    tree,
    (node) =>
      typeof node.type === 'function' && node.type.name === 'ModalPlaceholder',
  );
  assert.ok(placeholder, 'missing ModalPlaceholder component');
  return placeholder.type(placeholder.props);
}

test('controlled Modal exposes state, forwards changes, and honors defaultOpen', async () => {
  const ui = await loadModal();
  const changes = [];
  const controlled = ui.Modal({
    open: true,
    onOpenChange: (open) => changes.push(open),
    children: 'content',
  });
  const context = controlled.props.value;
  assert.equal(context.open, true);
  assert.equal(context.layoutId, undefined);
  assert.equal(typeof context.setOpen, 'function');
  assert.equal(typeof context.setLayoutId, 'function');
  assert.equal(typeof context.registerTrigger, 'function');
  assert.ok(context.originRef);

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

test('useModal shares the nearest controlled state', async () => {
  const ui = await loadModal();
  const changes = [];
  const owner = ui.Modal({
    open: true,
    onOpenChange: (open) => changes.push(open),
  });
  ui.setContext(owner.props.value);

  const modal = ui.useModal();
  assert.equal(modal.open, true);
  modal.setOpen(false);
  assert.deepEqual(changes, [false]);
});

test('triggers and persistent anchors select independent animation origins', async () => {
  const ui = await loadModal();
  const owner = ui.Modal({ open: false });
  const context = owner.props.value;
  ui.setContext(context);

  const triggerElement = { focus() {} };
  const trigger = ui.ModalTrigger({
    asChild: true,
    children: {
      type: 'button',
      props: { className: 'child-class', children: 'Launch' },
    },
  });
  const triggerNode = slot(trigger, 'modal-trigger');
  const triggerPlaceholder = slot(
    renderPlaceholder(trigger, ui),
    'modal-trigger-placeholder',
  );
  assert.equal(triggerNode.props['data-layout-id'], 'modal-trigger-1');
  assert.equal(triggerPlaceholder.props.layoutId, 'modal-trigger-1');
  assert.equal(triggerPlaceholder.props.layoutDependency, false);
  assert.equal(
    kind(trigger, 'Slot.Root').props.className,
    'relative child-class',
  );

  triggerNode.props.onClick({
    currentTarget: triggerElement,
    defaultPrevented: false,
  });
  assert.equal(context.originRef.current, triggerElement);

  const anchorTarget = new Element();
  anchorTarget.focus = () => {};
  const pointerTarget = new Element();
  pointerTarget.closest = () => anchorTarget;
  const anchorElement = new Element();
  anchorElement.contains = (target) => target === anchorTarget;
  const anchor = ui.ModalAnchor({ children: 'Menu anchor' });
  const anchorNode = slot(anchor, 'modal-anchor');
  const anchorPlaceholder = slot(
    renderPlaceholder(anchor, ui),
    'modal-trigger-placeholder',
  );
  assert.equal(anchorNode.props.className, 'relative');
  assert.equal(anchorPlaceholder.props.layoutId, 'modal-trigger-2');
  anchorNode.props.onPointerDown({
    currentTarget: anchorElement,
    target: pointerTarget,
    defaultPrevented: false,
  });
  assert.equal(context.originRef.current, anchorTarget);

  const focused = { focus() {} };
  anchorNode.props.onFocus({ target: focused, defaultPrevented: false });
  assert.equal(context.originRef.current, focused);
});

test('open content keeps its layout identity and can block dismissal', async () => {
  const ui = await loadModal();
  const owner = ui.Modal({ open: true });
  ui.setContext({ ...owner.props.value, layoutId: 'modal-layout-test-id' });

  const first = ui.ModalContent({
    children: 'First render',
    id: 'dialog',
    dismissible: false,
  });
  const second = ui.ModalContent({ children: 'Second render', id: 'dialog' });
  for (const tree of [first, second]) {
    const presence = kind(tree, 'AnimatePresence');
    assert.equal(presence.props.initial, false);
    assert.equal(kind(tree, 'Dialog.Portal').props.forceMount, true);
    const content = slot(tree, 'modal-content');
    assert.equal(content.props.id, 'dialog');
    assert.equal(content.props['data-layout-id'], 'modal-layout-test-id');
    const animated = kind(tree, 'motion.div');
    assert.equal(animated.props.layout, true);
    assert.equal(animated.props.layoutId, 'modal-layout-test-id');
    assert.equal(animated.props.layoutDependency, 'modal-content');
  }

  const content = slot(first, 'modal-content');
  const escape = {
    prevented: false,
    preventDefault() {
      this.prevented = true;
    },
  };
  const outside = {
    prevented: false,
    preventDefault() {
      this.prevented = true;
    },
  };
  content.props.onEscapeKeyDown(escape);
  content.props.onInteractOutside(outside);
  assert.equal(escape.prevented, true);
  assert.equal(outside.prevented, true);

  const closed = ui.Modal({ open: false });
  ui.setContext(closed.props.value);
  assert.equal(
    kind(ui.ModalContent({ children: 'Hidden' }), 'AnimatePresence').props
      .children,
    false,
  );
});

test('ModalClose forwards button props and retains its asChild API', async () => {
  const ui = await loadModal();
  assert.throws(
    () => ui.ModalTrigger({ children: 'Orphan' }),
    /outside the Modal/,
  );
  assert.throws(
    () => ui.ModalContent({ children: 'Orphan' }),
    /outside the Modal/,
  );

  const direct = ui.ModalClose({
    disabled: true,
    variant: 'flat',
    children: 'Dismiss',
  });
  assert.equal(direct.type, 'Dialog.Close');
  assert.equal(direct.props.asChild, true);
  const button = kind(direct, 'Button');
  assert.equal(button.props.disabled, true);
  assert.equal(button.props.variant, 'flat');
  assert.equal(button.props.children, 'Dismiss');

  const custom = ui.ModalClose({
    asChild: true,
    disabled: true,
    children: { type: 'button', props: { children: 'Custom control' } },
  });
  assert.equal(custom.type, 'Dialog.Close');
  assert.equal(kind(custom, 'Slot.Root').props.disabled, true);
});
