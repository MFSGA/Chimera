const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function setup() {
  const jsx = (type, props) => ({ type, props: props || {} });
  const imports = {
    '@chimera/utils': { cn: (...values) => values.filter(Boolean).join(' ') },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (next) => onChange?.(next),
      ],
    },
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { li: 'motion.li' },
    },
    'radix-ui': {
      Toast: {
        Provider: 'Toast.Provider',
        Viewport: 'Toast.Viewport',
        Root: 'Toast.Root',
      },
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const source = readFileSync(
    path.join(__dirname, '../src/snackbar.tsx'),
    'utf8',
  );
  const compiled = ts.transpileModule(source, {
    fileName: 'snackbar.tsx',
    reportDiagnostics: true,
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  });
  assert.equal(
    (compiled.diagnostics || []).filter(
      (item) => item.category === ts.DiagnosticCategory.Error,
    ).length,
    0,
  );
  const module = { exports: {} };
  new Function('require', 'module', 'exports', compiled.outputText)(
    (name) => {
      assert.ok(Object.hasOwn(imports, name), 'unexpected import: ' + name);
      return imports[name];
    },
    module,
    module.exports,
  );
  return module.exports;
}

function find(node, type) {
  if (!node || typeof node !== 'object') return null;
  if (node.type === type) return node;
  const children = node.props?.children;
  if (Array.isArray(children)) {
    for (const child of children) {
      const result = find(child, type);
      if (result) return result;
    }
    return null;
  }
  return find(children, type);
}

test('SnackbarProvider keeps children and mounts the bottom viewport', () => {
  const { SnackbarProvider } = setup();
  const tree = SnackbarProvider({
    duration: 4000,
    children: 'content',
  });
  assert.equal(tree.type, 'Toast.Provider');
  assert.equal(tree.props.duration, 4000);
  const viewport = find(tree, 'Toast.Viewport');
  assert.ok(viewport);
  assert.equal(viewport.props['data-slot'], 'snackbar-viewport');
  assert.match(viewport.props.className, /bottom-0/);
  assert.match(viewport.props.className, /pointer-events-none/);
});

test('Snackbar is closed by default and preserves defaultOpen', () => {
  const { Snackbar } = setup();
  const closed = Snackbar({ children: 'hidden' });
  assert.equal(closed.type, 'AnimatePresence');
  assert.equal(closed.props.children, false);
  const opened = Snackbar({ defaultOpen: true, children: 'visible' });
  const root = find(opened, 'Toast.Root');
  assert.ok(root);
  assert.equal(root.props.open, true);
  assert.equal(root.props.forceMount, true);
  assert.equal(root.props.asChild, true);
});

test('Snackbar forwards controlled state, props and exit animation', () => {
  const { Snackbar } = setup();
  const changes = [];
  const tree = Snackbar({
    open: true,
    onOpenChange: (value) => changes.push(value),
    duration: 6000,
    className: 'custom-toast',
    children: 'Saved',
  });
  const root = find(tree, 'Toast.Root');
  assert.equal(root.props.duration, 6000);
  root.props.onOpenChange(false);
  assert.deepEqual(changes, [false]);
  const surface = find(tree, 'motion.li');
  assert.ok(surface);
  assert.equal(surface.props['data-slot'], 'snackbar');
  assert.match(surface.props.className, /custom-toast/);
  assert.match(surface.props.className, /pointer-events-auto/);
  assert.deepEqual(surface.props.initial, { opacity: 0, y: 16 });
  assert.deepEqual(surface.props.animate, { opacity: 1, y: 0 });
  assert.deepEqual(surface.props.exit, { opacity: 0, y: 16 });
  assert.deepEqual(surface.props.transition, {
    duration: 0.2,
    ease: 'easeOut',
  });
});
