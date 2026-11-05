/**
 * Contracts for the pinned-reference FilterChip and Accordion components.
 * Real production TSX is transpiled; React/Radix/Motion module boundaries are
 * mocked. This is not browser focus, keyboard or desktop E2E evidence.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function fixture({ reducedMotion = false } = {}) {
  const contexts = [];
  const contextValues = new Map();
  const jsx = (type, props, key) => ({ type, props: props || {}, key });
  const modules = {
    '@chimera/utils': { cn: (...parts) => parts.filter(Boolean).join(' ') },
    '@radix-ui/react-use-controllable-state': {
      useControllableState: ({ prop, defaultProp, onChange }) => [
        prop ?? defaultProp,
        (next) => onChange?.(next),
      ],
    },
    '~icons/material-symbols/check-rounded': 'CheckRounded',
    '~icons/material-symbols/expand-more-rounded': 'ExpandMoreRounded',
    'motion/react': {
      AnimatePresence: 'AnimatePresence',
      motion: { span: 'motion.span', div: 'motion.div' },
      useReducedMotion: () => reducedMotion,
    },
    'radix-ui': {
      Toggle: { Root: 'Toggle.Root' },
      Accordion: {
        Root: 'Accordion.Root',
        Item: 'Accordion.Item',
        Header: 'Accordion.Header',
        Trigger: 'Accordion.Trigger',
        Content: 'Accordion.Content',
      },
    },
    react: {
      createContext: (defaultValue) => {
        const ctx = {
          defaultValue,
          Provider: 'Context.Provider.' + contexts.length,
        };
        contexts.push(ctx);
        return ctx;
      },
      useContext: (ctx) =>
        contextValues.has(ctx) ? contextValues.get(ctx) : ctx.defaultValue,
    },
    'react/jsx-runtime': { jsx, jsxs: jsx },
  };

  const load = (filename) => {
    const source = readFileSync(
      path.join(__dirname, '../src', filename + '.tsx'),
      'utf8',
    );
    const compiled = ts.transpileModule(source, {
      fileName: filename + '.tsx',
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
    const holder = { exports: {} };
    const execute = new Function(
      'require',
      'module',
      'exports',
      compiled.outputText,
    );
    execute(
      (name) => {
        assert.ok(Object.hasOwn(modules, name), 'Unexpected import ' + name);
        return modules[name];
      },
      holder,
      holder.exports,
    );
    return holder.exports;
  };
  return {
    load,
    setContext: (index, value) => contextValues.set(contexts[index], value),
    contexts,
  };
}

function find(node, predicate) {
  if (Array.isArray(node)) {
    for (const part of node) {
      const match = find(part, predicate);
      if (match) return match;
    }
    return null;
  }
  if (!node || typeof node !== 'object') return null;
  if (predicate(node)) return node;
  return find(node.props?.children, predicate);
}

function byType(node, type) {
  const found = find(node, (part) => part.type === type);
  assert.ok(found, 'Expected ' + type);
  return found;
}

function bySlot(node, slot) {
  const found = find(node, (part) => part.props?.['data-slot'] === slot);
  assert.ok(found, 'Expected data-slot ' + slot);
  return found;
}

test('FilterChip defaults off and forwards controlled toggle callback', () => {
  const { FilterChip } = fixture().load('chip');
  const changes = [];
  const tree = FilterChip({
    pressed: false,
    onPressedChange: (value) => changes.push(value),
    className: 'extra-chip',
    'aria-label': 'Filter results',
    children: 'Hide unavailable',
  });
  assert.equal(tree.type, 'Toggle.Root');
  assert.equal(tree.props.pressed, false);
  assert.equal(tree.props['data-slot'], 'filter-chip');
  assert.equal(tree.props['aria-label'], 'Filter results');
  assert.match(tree.props.className, /extra-chip/);
  assert.match(tree.props.className, /focus-visible:outline-primary/);
  assert.equal(byType(tree, 'AnimatePresence').props.initial, false);
  assert.equal(find(tree, (part) => part.type === 'motion.span'), null);
  tree.props.onPressedChange(true);
  assert.deepEqual(changes, [true]);
});

test('FilterChip renders selected check with motion and reduced-motion', () => {
  for (const reducedMotion of [false, true]) {
    const { FilterChip } = fixture({ reducedMotion }).load('chip');
    const tree = FilterChip({ pressed: true, children: 'Active filter' });
    const check = bySlot(tree, 'filter-chip-check');
    assert.equal(check.type, 'motion.span');
    assert.equal(check.props['aria-hidden'], true);
    assert.equal(
      byType(check, 'CheckRounded').props.className,
      'size-[18px] shrink-0',
    );
    assert.deepEqual(check.props.initial, {
      width: 0,
      opacity: 0,
      marginRight: -8,
    });
    assert.equal(check.props.animate.width, 18);
    assert.equal(check.props.exit.opacity, 0);
    assert.deepEqual(
      check.props.transition,
      reducedMotion
        ? { duration: 0 }
        : { duration: 0.2, ease: 'easeOut' },
    );
  }
});

test('FilterChip preserves defaultPressed and disabled primitive props', () => {
  const { FilterChip } = fixture().load('chip');
  const tree = FilterChip({
    defaultPressed: true,
    disabled: true,
    children: 'Unavailable',
    title: 'Status',
  });
  assert.equal(tree.props.pressed, true);
  assert.equal(tree.props.disabled, true);
  assert.equal(tree.props.title, 'Status');
  assert.equal(bySlot(tree, 'filter-chip-check').type, 'motion.span');
});

test('Accordion forces single collapsible state and forwards changes', () => {
  const env = fixture();
  const { Accordion } = env.load('accordion');
  const changes = [];
  const controlled = Accordion({
    value: 'network',
    onValueChange: (value) => changes.push(value),
    className: 'my-accordion',
    dir: 'ltr',
  });
  assert.equal(controlled.type, 'Context.Provider.0');
  assert.equal(controlled.props.value, 'network');
  const root = byType(controlled, 'Accordion.Root');
  assert.equal(root.props.value, 'network');
  assert.equal(root.props.type, 'single');
  assert.equal(root.props.collapsible, true);
  assert.equal(root.props.dir, 'ltr');
  assert.match(root.props.className, /my-accordion/);
  root.props.onValueChange('');
  assert.deepEqual(changes, ['']);
  assert.equal(Accordion({}).props.value, '');
  assert.equal(Accordion({ defaultValue: 'details' }).props.value, 'details');
});

test('Accordion expanded item retains chevron and content animation', () => {
  const env = fixture();
  const ui = env.load('accordion');
  env.setContext(0, 'account');
  const item = ui.AccordionItem({ value: 'account', className: 'item-1' });
  assert.equal(item.type, 'Context.Provider.1');
  assert.equal(item.props.value.open, true);
  assert.equal(bySlot(item, 'accordion-item').props.value, 'account');
  env.setContext(1, item.props.value);
  const trigger = ui.AccordionTrigger({
    className: 'trigger-class',
    children: 'Account',
    id: 'account-trigger',
  });
  assert.equal(
    bySlot(trigger, 'accordion-trigger').props.id,
    'account-trigger',
  );
  assert.equal(bySlot(trigger, 'accordion-header').type, 'Accordion.Header');
  assert.equal(bySlot(trigger, 'accordion-chevron').props.animate.rotate, 180);
  assert.equal(
    bySlot(trigger, 'accordion-chevron').props.transition.duration,
    0.2,
  );
  const content = ui.AccordionContent({
    children: 'Account settings',
    className: 'details',
    id: 'account-content',
  });
  assert.equal(byType(content, 'AnimatePresence').props.initial, false);
  const primitive = byType(content, 'Accordion.Content');
  assert.equal(primitive.props.forceMount, true);
  assert.equal(primitive.props.asChild, true);
  assert.equal(primitive.props.id, 'account-content');
  const motion = bySlot(content, 'accordion-content');
  assert.equal(motion.props.animate.height, 'auto');
  assert.equal(motion.props.exit.height, 0);
  assert.equal(motion.props.transition.height.duration, 0.25);
  assert.match(byType(motion, 'div').props.className, /details/);
});

test('Accordion closed item and reduced-motion behavior', () => {
  const env = fixture({ reducedMotion: true });
  const ui = env.load('accordion');
  env.setContext(0, 'account');
  const closed = ui.AccordionItem({ value: 'network' });
  assert.equal(closed.props.value.open, false);
  env.setContext(1, closed.props.value);
  const trigger = ui.AccordionTrigger({ children: 'Network' });
  assert.deepEqual(bySlot(trigger, 'accordion-chevron').props.transition, {
    duration: 0,
  });
  assert.equal(bySlot(trigger, 'accordion-chevron').props.animate.rotate, 0);
  const hidden = ui.AccordionContent({ children: 'Hidden settings' });
  assert.equal(byType(hidden, 'AnimatePresence').props.children, false);
  env.setContext(1, { open: true });
  const revealed = ui.AccordionContent({ children: 'Open settings' });
  assert.deepEqual(bySlot(revealed, 'accordion-content').props.transition, {
    duration: 0,
  });
});
