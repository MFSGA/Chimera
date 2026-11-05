/**
 * Production-source Node contracts for ref-aligned SearchField and the
 * intentional dynamic-label exception in Input. These do not simulate native
 * browser focus, layout, pointer input or keyboard accessibility end-to-end.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function source(name) {
  return readFileSync(path.join(__dirname, '../src', name), 'utf8');
}

function transpile(name) {
  const compiled = ts.transpileModule(source(name), {
    fileName: name,
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
  return compiled.outputText;
}

function makeClassVariants(base, settings = {}) {
  const classes = (input) =>
    (Array.isArray(input) ? input.flat(Infinity) : [input])
      .filter((value) => typeof value === 'string' && value.length > 0)
      .join(' ');
  return (props = {}) => {
    const variants = Object.keys(settings.variants || {}).flatMap((name) => {
      const selected = props[name] ?? settings.defaultVariants?.[name];
      return settings.variants[name][selected] || [];
    });
    return classes([base, ...variants]);
  };
}

function execute(name, dependencies) {
  const module = { exports: {} };
  const run = new Function('require', 'module', 'exports', transpile(name));
  run((id) => {
    assert.ok(Object.hasOwn(dependencies, id), 'unexpected import: ' + id);
    return dependencies[id];
  }, module, module.exports);
  return module.exports;
}

function makeFixture() {
  let focuses = 0;
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const mocks = {
    '@chimera/utils': { cn: (...values) => values.filter(Boolean).join(' ') },
    'class-variance-authority': { cva: makeClassVariants },
    react: {
      createContext: (defaultValue) => ({ defaultValue }),
      useRef: () => ({
        current: {
          focus: () => {
            focuses += 1;
          },
        },
      }),
      useImperativeHandle: (forwarded, factory) => {
        if (forwarded) forwarded.current = factory();
      },
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
    '~icons/material-symbols/close-rounded': () => null,
    '~icons/material-symbols/search-rounded': () => null,
  };
  const glass = execute('glass-surface.tsx', mocks);
  mocks['./glass-surface'] = glass;
  const search = execute('search-field.tsx', mocks);
  return { ...search, glass, focuses: () => focuses };
}

function find(node, predicate) {
  if (Array.isArray(node)) {
    for (const item of node) {
      const found = find(item, predicate);
      if (found) return found;
    }
    return null;
  }
  if (node && typeof node === 'object') {
    if (predicate(node)) return node;
    return find(node.props?.children, predicate);
  }
  return null;
}

function slot(node, name) {
  const match = find(node, (item) => item.props?.['data-slot'] === name);
  assert.ok(match, 'missing ' + name);
  return match;
}

test('basic search retains input changes and default appearance', () => {
  const ui = makeFixture();
  const changes = [];
  const tree = ui.SearchField({
    value: 'hello',
    clearLabel: 'Clear search',
    placeholder: 'Search widgets',
    onValueChange: (value) => changes.push(value),
    className: 'custom-search',
  });
  assert.equal(tree.props['data-slot'], 'search-field');
  assert.match(tree.props.className, /h-10/);
  assert.match(tree.props.className, /custom-search/);
  assert.doesNotMatch(tree.props.className, /shadow-lg/);
  const input = slot(tree, 'search-field-input');
  assert.equal(input.props.type, 'search');
  assert.equal(input.props.value, 'hello');
  assert.equal(input.props['aria-label'], 'Search widgets');
  input.props.onChange({ target: { value: 'updated' } });
  assert.deepEqual(changes, ['updated']);
});

test('glass variant shares glass-surface styling', () => {
  const ui = makeFixture();
  const glass = ui.SearchField({
    value: '',
    clearLabel: 'Clear',
    variant: 'glass',
    onValueChange: () => {},
  });
  assert.match(glass.props.className, /shadow-lg/);
  assert.match(glass.props.className, /shadow-shadow\/15/);
  assert.match(glass.props.className, /h-11/);
  assert.match(glass.props.className, /focus-within:outline-primary/);
  assert.equal(ui.glass.GlassControlGroupContext.defaultValue, false);
});

test('Escape, clear and forwarded imperative ref keep search UX', () => {
  const ui = makeFixture();
  const keys = [];
  const updates = [];
  const forwarded = { current: null };
  const tree = ui.SearchField({
    value: 'query',
    clearLabel: 'Erase',
    ref: forwarded,
    onKeyDown: (e) => keys.push(e.key),
    onValueChange: (value) => updates.push(value),
  });
  const input = slot(tree, 'search-field-input');
  input.props.onKeyDown({ key: 'ArrowRight' });
  assert.deepEqual(keys, ['ArrowRight']);
  assert.deepEqual(updates, []);
  input.props.onKeyDown({ key: 'Escape' });
  assert.deepEqual(keys, ['ArrowRight', 'Escape']);
  assert.deepEqual(updates, ['']);
  const clear = slot(tree, 'search-field-clear');
  assert.equal(clear.type, 'button');
  assert.equal(clear.props.type, 'button');
  assert.equal(clear.props['aria-label'], 'Erase');
  clear.props.onClick();
  assert.deepEqual(updates, ['', '']);
  assert.equal(ui.focuses(), 1);
  assert.equal(typeof forwarded.current?.focus, 'function');
});

test('read-only and disabled prevent clearing', () => {
  const ui = makeFixture();
  for (const key of ['readOnly', 'disabled']) {
    const updates = [];
    const tree = ui.SearchField({
      value: 'important',
      clearLabel: 'Clear',
      [key]: true,
      'aria-labelledby': 'search-caption',
      onValueChange: (value) => updates.push(value),
    });
    const input = slot(tree, 'search-field-input');
    assert.equal(input.props['aria-label'], undefined);
    assert.equal(input.props['aria-labelledby'], 'search-caption');
    assert.equal(input.props[key], true);
    assert.equal(
      find(tree, (item) => item.props?.['data-slot'] === 'search-field-clear'),
      null,
    );
    if (key === 'readOnly') {
      input.props.onKeyDown({ key: 'Escape' });
      assert.deepEqual(updates, []);
    }
  }
});

test('Input retains dynamic floating-label dependencies', () => {
  const file = ts.createSourceFile(
    'input.tsx',
    source('input.tsx'),
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  const dependencies = [];
  function walk(node) {
    if (
      ts.isCallExpression(node) &&
      ts.isIdentifier(node.expression) &&
      node.expression.text === 'useMemo' &&
      node.arguments.length === 2 &&
      ts.isArrayLiteralExpression(node.arguments[1])
    ) {
      dependencies.push(
        node.arguments[1].elements.map((item) => item.getText(file)),
      );
    }
    ts.forEachChild(node, walk);
  }
  walk(file);
  assert.deepEqual(dependencies, [['label', 'children']]);
});
