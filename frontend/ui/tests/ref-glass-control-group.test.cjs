/**
 * Node contracts for production GlassControlGroup and Button components.
 * React, Radix Slot, Motion and CVA are mocked at module boundaries; these
 * checks do not replace browser layout, actual focus or pointer interaction.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function compile(name) {
  const content = readFileSync(path.join(__dirname, '../src', name), 'utf8');
  const compiled = ts.transpileModule(content, {
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
  );
  return compiled.outputText;
}

function flatten(values) {
  return (Array.isArray(values) ? values.flat(Infinity) : [values])
    .filter((value) => typeof value === 'string' && value.length > 0)
    .join(' ');
}

function cva(base, options = {}) {
  return (props = {}) => {
    const selected = Object.fromEntries(
      Object.keys(options.variants || {}).map((name) => [
        name,
        props[name] ?? options.defaultVariants?.[name],
      ]),
    );
    const variants = Object.entries(options.variants || {}).map(
      ([name, values]) => values[String(selected[name])],
    );
    const compounds = (options.compoundVariants || [])
      .filter((variant) =>
        Object.entries(variant)
          .filter(([key]) => key !== 'className')
          .every(([key, value]) => selected[key] === value),
      )
      .map((variant) => variant.className);
    return flatten([base, variants, compounds]);
  };
}

function fixture() {
  const contexts = new Map();
  const state = { rippleClicks: 0, clicks: 0 };
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const mocks = {
    '@chimera/utils': {
      cn: (...classes) => flatten(classes),
      chains: (...callbacks) => (event) => {
        for (const callback of callbacks) callback?.(event);
      },
    },
    'class-variance-authority': { cva },
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { span: 'motion.span' },
    },
    'radix-ui': { Slot: { Root: 'Slot.Root', Slottable: 'Slot.Slottable' } },
    react: {
      createContext: (defaultValue) => ({
        defaultValue,
        Provider: 'GlassContext.Provider',
      }),
      useContext: (ctx) =>
        contexts.has(ctx) ? contexts.get(ctx) : ctx.defaultValue,
      cloneElement: (element, props, children) => ({
        ...element,
        props: { ...element.props, ...props, children },
      }),
      isValidElement: (element) =>
        !!element && typeof element === 'object' && !!element.type,
      lazy: () => 'LazyRipple',
      Suspense: 'Suspense',
      useCallback: (callback) => callback,
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
    './progress': { CircularProgress: 'CircularProgress' },
    './ripple': {
      useRipple: () => ({
        onClick: () => {
          state.rippleClicks += 1;
        },
        onClear: () => {},
        ripples: [],
      }),
    },
  };
  const load = (name) => {
    const module = { exports: {} };
    const evaluate = new Function(
      'require',
      'module',
      'exports',
      compile(name),
    );
    evaluate((specifier) => {
      assert.ok(Object.hasOwn(mocks, specifier), 'unknown import ' + specifier);
      return mocks[specifier];
    }, module, module.exports);
    return module.exports;
  };
  const glass = load('glass-surface.tsx');
  mocks['./glass-surface'] = glass;
  const group = load('glass-control-group.tsx');
  const button = load('button.tsx');
  return {
    glass,
    group,
    button,
    state,
    setGrouped: (value) => contexts.set(glass.GlassControlGroupContext, value),
  };
}

function find(tree, predicate) {
  if (Array.isArray(tree)) {
    for (const child of tree) {
      const match = find(child, predicate);
      if (match) return match;
    }
  } else if (tree && typeof tree === 'object') {
    if (predicate(tree)) return tree;
    return find(tree.props?.children, predicate);
  }
  return null;
}

function slot(tree, name) {
  const node = find(tree, (item) => item.props?.['data-slot'] === name);
  assert.ok(node, 'missing ' + name);
  return node;
}

test('GlassControlGroup forwards context, style and props', () => {
  const { glass, group } = fixture();
  assert.equal(glass.GlassControlGroupContext.defaultValue, false);
  const result = group.GlassControlGroup({
    className: 'extra',
    'aria-label': 'Window tools',
    children: 'Actions',
  });
  assert.equal(result.type, 'GlassContext.Provider');
  assert.equal(result.props.value, true);
  const div = slot(result, 'glass-control-group');
  assert.equal(div.type, 'div');
  assert.equal(div.props['aria-label'], 'Window tools');
  assert.equal(div.props.children, 'Actions');
  assert.match(div.props.className, /shadow-lg/);
  assert.match(div.props.className, /\[&>button\]:h-9/);
  assert.match(div.props.className, /extra/);
});

test('glass buttons share their surface only when grouped', () => {
  const ui = fixture();
  const standalone = ui.button.Button({ variant: 'glass', children: 'Open' });
  assert.match(standalone.props.className, /shadow-lg/);
  assert.match(standalone.props.className, /h-11/);
  assert.doesNotMatch(
    standalone.props.className,
    /shrink-0 items-center gap-1/,
  );
  ui.setGrouped(true);
  const grouped = ui.button.Button({
    variant: 'glass',
    children: 'Open',
    className: 'custom-item',
  });
  assert.doesNotMatch(grouped.props.className, /shadow-lg/);
  assert.match(grouped.props.className, /h-9 min-w-0 shrink-0/);
  assert.match(grouped.props.className, /custom-item/);
  assert.equal(slot(grouped, 'button-content').props.children, 'Open');
  const icon = ui.button.Button({ variant: 'glass', icon: true });
  assert.match(icon.props.className, /w-10/);
  assert.doesNotMatch(icon.props.className, /w-11/);
  ui.setGrouped(false);
  const iconOutside = ui.button.Button({ variant: 'glass', icon: true });
  assert.match(iconOutside.props.className, /w-11/);
});

test('click, disableRipple and disabled contracts remain intact', () => {
  const ui = fixture();
  const callback = () => {
    ui.state.clicks += 1;
  };
  const normal = ui.button.Button({ variant: 'basic', onClick: callback });
  normal.props.onClick({});
  assert.equal(ui.state.clicks, 1);
  assert.equal(ui.state.rippleClicks, 1);
  const noRipple = ui.button.Button({
    variant: 'raised',
    disableRipple: true,
    onClick: callback,
  });
  noRipple.props.onClick({});
  assert.equal(ui.state.clicks, 2);
  assert.equal(ui.state.rippleClicks, 1);
  const disabled = ui.button.Button({
    variant: 'glass',
    disabled: true,
    onClick: callback,
  });
  assert.equal(disabled.props.onClick, undefined);
  assert.equal(disabled.props.disabled, true);
  assert.equal(disabled.props['data-loading'], 'false');
  assert.equal(ui.state.clicks, 2);
});

test('asChild keeps child attributes and loading content', () => {
  const ui = fixture();
  const link = {
    type: 'a',
    props: {
      href: '#details',
      children: 'Details',
      'aria-label': 'Details link',
    },
  };
  const result = ui.button.Button({
    asChild: true,
    loading: true,
    disableRipple: true,
    children: link,
  });
  assert.equal(result.type, 'Slot.Root');
  const slottable = find(result, (node) => node.type === 'Slot.Slottable');
  assert.ok(slottable);
  const wrapped = slottable.props.children;
  assert.equal(wrapped.type, 'a');
  assert.equal(wrapped.props.href, '#details');
  assert.equal(wrapped.props['aria-label'], 'Details link');
  assert.equal(slot(wrapped, 'button-content').props.children, 'Details');
  assert.equal(slot(result, 'button-loading').type, 'motion.span');
  assert.match(result.props.className, /button-loading.*button-content/);
  const direct = ui.button.Button({ loading: false, children: 'Click' });
  assert.equal(direct.type, 'button');
  assert.equal(slot(direct, 'button-content').props.children, 'Click');
});
