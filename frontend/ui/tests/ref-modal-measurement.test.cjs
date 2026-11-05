/**
 * AST contracts for reference Modal shared-layout measurements. Browser
 * animation, focus management and accessibility still require desktop E2E.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

const filename = path.join(__dirname, '../src/modal.tsx');
const text = readFileSync(filename, 'utf8');
const file = ts.createSourceFile(
  filename,
  text,
  ts.ScriptTarget.Latest,
  true,
  ts.ScriptKind.TSX,
);

function functionNamed(name) {
  const found = file.statements.find(
    (node) => ts.isFunctionDeclaration(node) && node.name?.text === name,
  );
  assert.ok(found, 'missing ' + name);
  return found;
}

function attr(node, name) {
  return node.attributes.properties.find(
    (item) => ts.isJsxAttribute(item) && item.name.text === name,
  );
}

function descendants(node, predicate) {
  const found = [];
  function visit(item) {
    if (predicate(item)) found.push(item);
    ts.forEachChild(item, visit);
  }
  visit(node);
  return found;
}

test('trigger measures placeholder when open changes', () => {
  const trigger = functionNamed('ModalTrigger');
  const placeholders = descendants(
    trigger,
    (node) =>
      ts.isJsxSelfClosingElement(node) &&
      attr(node, 'data-slot')?.initializer?.getText(file) ===
        '"modal-trigger-placeholder"',
  );
  assert.equal(placeholders.length, 1);
  const placeholder = placeholders[0];
  assert.equal(
    attr(placeholder, 'layoutDependency')?.initializer?.getText(file),
    '{open}',
  );
  assert.ok(attr(placeholder, 'layout'));
  assert.equal(
    attr(placeholder, 'layoutId')?.initializer?.getText(file),
    '{layoutId}',
  );
  assert.match(
    trigger.getText(file),
    /const \{ open, layoutId \} = useModalContext\(\)/,
  );
});

test('dialog content keeps stable shared-layout measurement dependency', () => {
  const content = functionNamed('ModalContent');
  const nodes = descendants(
    content,
    (node) =>
      ts.isJsxOpeningElement(node) &&
      node.tagName.getText(file) === 'motion.div' &&
      !!attr(node, 'layoutDependency'),
  );
  assert.equal(nodes.length, 1);
  assert.equal(
    attr(nodes[0], 'layoutDependency')?.initializer?.getText(file),
    '"modal-content"',
  );
  assert.ok(attr(nodes[0], 'layout'));
  assert.equal(
    attr(nodes[0], 'layoutId')?.initializer?.getText(file),
    '{layoutId}',
  );
});

test('controlled state, close button and context guards stay intact', () => {
  const modal = functionNamed('Modal').getText(file);
  const close = functionNamed('ModalClose').getText(file);
  assert.match(modal, /useControllableState\(/);
  assert.match(modal, /onChange: onOpenChange/);
  assert.match(modal, /DialogPrimitive\.Root open=\{open\}/);
  assert.match(close, /DialogPrimitive\.Close/);
  assert.match(close, /asChild \? children : <Button>/);
  assert.match(text, /Modal compound components must be rendered inside Modal/);
});

test('modal source transpiles as TSX without TypeScript syntax errors', () => {
  const result = ts.transpileModule(text, {
    fileName: filename,
    reportDiagnostics: true,
    compilerOptions: {
      jsx: ts.JsxEmit.ReactJSX,
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
    },
  });
  assert.deepEqual(
    (result.diagnostics ?? []).filter(
      (item) => item.category === ts.DiagnosticCategory.Error,
    ),
    [],
  );
});
