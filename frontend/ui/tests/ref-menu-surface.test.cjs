const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function load(reducedMotion) {
  const source = readFileSync(
    path.join(__dirname, '../src/menu-surface.tsx'),
    'utf8',
  );
  const compiled = ts.transpileModule(source, {
    fileName: 'menu-surface.tsx',
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  });
  const jsx = (type, props) => ({ type, props });
  const modules = {
    '@chimera/utils': { cn: (...values) => values.filter(Boolean).join(' ') },
    'class-variance-authority': {
      cva: (classes) => () => classes.join(' '),
    },
    'motion/react': {
      motion: {
        create: (root) => 'motion.create:' + root,
        div: 'motion.div',
      },
      useReducedMotion: () => reducedMotion,
    },
    'radix-ui': { Slot: { Root: 'Slot.Root' } },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };
  const module = { exports: {} };
  const execute = new Function(
    'require',
    'module',
    'exports',
    compiled.outputText,
  );
  execute(
    (name) => {
      assert.ok(Object.hasOwn(modules, name), name);
      return modules[name];
    },
    module,
    module.exports,
  );
  return module.exports.MenuSurface;
}

test('MenuSurface uses a div with ref menu motion and popper geometry', () => {
  const surface = load(false)({
    children: 'Menu',
    className: 'extra',
    id: 'menu-one',
    style: { color: 'red' },
  });
  assert.equal(surface.type, 'motion.div');
  assert.equal(surface.props.id, 'menu-one');
  assert.equal(surface.props.children, 'Menu');
  assert.match(surface.props.className, /extra/);
  assert.match(surface.props.className, /bg-inverse-on-surface/);
  assert.equal(surface.props.style.color, 'red');
  assert.equal(
    surface.props.style.maxHeight,
    'var(--radix-popper-available-height)',
  );
  assert.match(surface.props.style.transformOrigin, /radix-menubar/);
  assert.match(surface.props.style.transformOrigin, /radix-dropdown-menu/);
  assert.deepEqual(surface.props.initial, { opacity: 0, scaleY: 0.9 });
  assert.deepEqual(surface.props.animate, { opacity: 1, scaleY: 1 });
  assert.deepEqual(surface.props.exit, { opacity: 0, scaleY: 0.9 });
  assert.equal(surface.props.transition.duration, 0.35);
});

test('MenuSurface composes asChild and respects reduced motion', () => {
  const surface = load(true)({
    asChild: true,
    children: 'Action',
    'aria-label': 'Actions',
  });
  assert.equal(surface.type, 'motion.create:Slot.Root');
  assert.equal(surface.props['aria-label'], 'Actions');
  assert.equal(surface.props.initial, false);
  assert.deepEqual(surface.props.transition, { duration: 0 });
});
