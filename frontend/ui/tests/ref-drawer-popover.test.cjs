/**
 * Reference Drawer/Popover lifecycle contract tests.
 * Transpiles production TSX and mocks React/Radix/Motion boundaries; actual
 * focus trapping, Escape, pointer and compositor behavior require browser E2E.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function makeFixture(name, { open = false, reducedMotion = false } = {}) {
  const changes = [];
  let activeOpen = open;
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const mocks = {
    '@chimera/utils': {
      cn: (...values) => values.filter(Boolean).join(' '),
    },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (next) => {
          changes.push(next);
          onChange?.(next);
        },
      ],
    },
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { div: 'motion.div' },
      useReducedMotion: () => reducedMotion,
    },
    'radix-ui': {
      Dialog: Object.fromEntries(
        ['Root', 'Title', 'Close', 'Portal', 'Overlay', 'Content'].map(
          (key) => [key, 'Dialog.' + key],
        ),
      ),
      Popover: Object.fromEntries(
        ['Root', 'Trigger', 'Anchor', 'Portal', 'Content'].map(
          (key) => [key, 'Popover.' + key],
        ),
      ),
    },
    react: {
      createContext: (defaultValue) => ({
        defaultValue,
        Provider: 'Context.Provider',
      }),
      useContext: () => activeOpen,
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };

  const file = path.join(__dirname, '../src', name + '.tsx');
  const source = readFileSync(file, 'utf8');
  const privateName = name === 'drawer' ? 'DrawerSurface' : 'PopoverSurface';
  const compiled = ts.transpileModule(
    source + '\nexport const __testSurface = ' + privateName + ';',
    {
      fileName: file,
      reportDiagnostics: true,
      compilerOptions: {
        jsx: ts.JsxEmit.ReactJSX,
        module: ts.ModuleKind.CommonJS,
        target: ts.ScriptTarget.ES2022,
        esModuleInterop: true,
      },
    },
  );
  assert.deepEqual(
    (compiled.diagnostics || []).filter(
      (diagnostic) => diagnostic.category === ts.DiagnosticCategory.Error,
    ),
    [],
    name + ' should transpile',
  );

  const module = { exports: {} };
  const execute = new Function(
    'require',
    'module',
    'exports',
    compiled.outputText,
  );
  execute(
    (id) => {
      assert.ok(Object.hasOwn(mocks, id), 'unexpected import ' + id);
      return mocks[id];
    },
    module,
    module.exports,
  );
  return {
    ui: module.exports,
    changes,
    setOpen: (next) => {
      activeOpen = next;
    },
  };
}

function find(tree, predicate) {
  if (Array.isArray(tree)) {
    for (const node of tree) {
      const found = find(node, predicate);
      if (found) return found;
    }
    return null;
  }
  if (!tree || typeof tree !== 'object') return null;
  if (predicate(tree)) return tree;
  return find(tree.props?.children, predicate);
}

function kind(tree, name) {
  const node = find(tree, (item) => item.type === name);
  assert.ok(node, 'missing ' + name);
  return node;
}

test('Drawer retains controlled/uncontrolled Radix state and callbacks', () => {
  const f = makeFixture('drawer');
  const received = [];
  const tree = f.ui.Drawer({
    open: true,
    onOpenChange: (next) => received.push(next),
    modal: true,
  });
  assert.equal(tree.type, 'Context.Provider');
  assert.equal(tree.props.value, true);
  const root = kind(tree, 'Dialog.Root');
  assert.equal(root.props.open, true);
  assert.equal(root.props.modal, true);
  root.props.onOpenChange(false);
  assert.deepEqual(received, [false]);
  assert.deepEqual(f.changes, [false]);
  assert.equal(f.ui.Drawer({ defaultOpen: true }).props.value, true);
  assert.equal(f.ui.Drawer({}).props.value, false);
  assert.equal(f.ui.DrawerTitle, 'Dialog.Title');
  assert.equal(f.ui.DrawerClose, 'Dialog.Close');
});

test('Drawer exit portal and overlay remain mounted during animation', () => {
  const f = makeFixture('drawer', { open: true });
  const tree = f.ui.DrawerContent({
    children: 'Controls',
    className: 'custom-panel',
    'aria-label': 'Settings',
  });
  const portal = kind(tree, 'Dialog.Portal');
  const overlay = kind(tree, 'Dialog.Overlay');
  const content = kind(tree, 'Dialog.Content');
  assert.equal(portal.props.forceMount, true);
  assert.equal(overlay.props.asChild, true);
  assert.equal(overlay.props.forceMount, true);
  assert.equal(content.props.forceMount, true);
  assert.equal(content.props.asChild, true);
  assert.equal(content.props['aria-label'], 'Settings');
  assert.equal(kind(tree, 'AnimatePresence').type, 'AnimatePresence');
  assert.equal(
    find(tree, (node) => node.props?.['data-slot'] === 'drawer-overlay').type,
    'motion.div',
  );
  assert.equal(content.props.children.props.children, 'Controls');
  f.setOpen(false);
  assert.equal(
    f.ui.DrawerContent({ children: 'Closed' }).props.children,
    false,
  );
});

test('Drawer inert, drag handle and reduced-motion presentation contracts', () => {
  for (const reducedMotion of [false, true]) {
    const f = makeFixture('drawer', { open: true, reducedMotion });
    const panel = f.ui.__testSurface({
      className: 'custom',
      children: 'Panel',
    });
    assert.equal(panel.props.inert, false);
    assert.equal(panel.props['aria-hidden'], undefined);
    assert.match(panel.props.className, /custom/);
    assert.deepEqual(
      panel.props.transition,
      reducedMotion
        ? { duration: 0 }
        : { type: 'spring', stiffness: 380, damping: 40 },
    );
    const overlay = find(
      f.ui.DrawerContent({ children: 'Panel' }),
      (node) => node.props?.['data-slot'] === 'drawer-overlay',
    );
    assert.equal(overlay.props.transition.duration, reducedMotion ? 0 : 0.18);
    const handle = f.ui.DrawerHandle({ id: 'grip' });
    assert.equal(handle.type, 'span');
    assert.equal(handle.props['aria-hidden'], true);
    assert.equal(handle.props.id, 'grip');
    f.setOpen(false);
    const closed = f.ui.__testSurface({});
    assert.equal(closed.props.inert, true);
    assert.equal(closed.props['aria-hidden'], true);
  }
});

test('Popover preserves state, Trigger and Anchor DOM contracts', () => {
  const f = makeFixture('popover');
  const received = [];
  const tree = f.ui.Popover({
    defaultOpen: true,
    onOpenChange: (value) => received.push(value),
  });
  assert.equal(tree.props.value, true);
  const root = kind(tree, 'Popover.Root');
  assert.equal(root.props.open, true);
  root.props.onOpenChange(false);
  assert.deepEqual(received, [false]);
  const trigger = f.ui.PopoverTrigger({ children: 'Choose', id: 'open-menu' });
  assert.equal(trigger.type, 'Popover.Trigger');
  assert.equal(trigger.props['data-slot'], 'popover-trigger');
  assert.equal(trigger.props.id, 'open-menu');
  assert.equal(f.ui.PopoverAnchor({ id: 'anchor' }).props.id, 'anchor');
  assert.equal(f.ui.Popover({}).props.value, false);
});

test('Popover portal keeps collision and focus positioning props', () => {
  const f = makeFixture('popover', { open: true });
  const tree = f.ui.PopoverContent({
    children: 'Options',
    side: 'top',
    align: 'end',
    sideOffset: 8,
    collisionPadding: 20,
    'aria-label': 'Choose option',
  });
  const portal = kind(tree, 'Popover.Portal');
  const content = kind(tree, 'Popover.Content');
  assert.equal(portal.props.forceMount, true);
  assert.equal(content.props.forceMount, true);
  assert.equal(content.props.asChild, true);
  assert.equal(content.props.side, 'top');
  assert.equal(content.props.align, 'end');
  assert.equal(content.props.sideOffset, 8);
  assert.equal(content.props.collisionPadding, 20);
  assert.equal(content.props['aria-label'], 'Choose option');
  const defaults = kind(f.ui.PopoverContent({}), 'Popover.Content');
  assert.deepEqual(
    [defaults.props.side, defaults.props.align, defaults.props.sideOffset],
    ['bottom', 'center', 16],
  );
  f.setOpen(false);
  assert.equal(f.ui.PopoverContent({}).props.children, false);
});

test('Popover surface applies inert, size, motion and top/bottom origin', () => {
  for (const reducedMotion of [false, true]) {
    const f = makeFixture('popover', { open: true, reducedMotion });
    const top = f.ui.__testSurface({
      children: 'Top content',
      'data-side': 'top',
      style: { opacity: 0.9 },
    });
    const bottom = f.ui.__testSurface({ 'data-side': 'bottom' });
    assert.equal(top.props.inert, false);
    assert.equal(top.props['aria-hidden'], undefined);
    assert.equal(top.props.style.maxWidth, 'calc(100vw - 24px)');
    assert.equal(top.props.style.opacity, 0.9);
    assert.equal(
      top.props.style.maxHeight,
      'var(--radix-popover-content-available-height)',
    );
    if (reducedMotion) {
      assert.deepEqual(top.props.initial, { opacity: 0 });
      assert.deepEqual(top.props.transition.duration, 0);
    } else {
      assert.notEqual(
        top.props.initial.clipPath,
        bottom.props.initial.clipPath,
      );
      assert.equal(top.props.transition.duration, 0.26);
    }
    assert.equal(kind(top, 'motion.div').props['data-slot'], 'popover-content');
    f.setOpen(false);
    const closed = f.ui.__testSurface({ 'data-side': 'bottom' });
    assert.equal(closed.props.inert, true);
    assert.equal(closed.props['aria-hidden'], true);
    assert.match(closed.props.className, /pointer-events-none/);
    assert.equal(closed.props.transition.duration, reducedMotion ? 0 : 0.18);
  }
});
