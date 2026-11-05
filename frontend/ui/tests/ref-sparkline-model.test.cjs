/**
 * Executes the production Sparkline model with a stubbed chart boundary.
 * Browser geometry, ResizeObserver delivery and compositor timing still require
 * upstream browser tests; these tests exercise continuation/animation semantics.
 */
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const ts = require('typescript');

function loadModel() {
  // Export only for this test by appending to the in-memory transpilation;
  // do not add a test-only export to the production component or change ref.
  const source = readFileSync(path.join(__dirname, '../src/sparkline.tsx'), 'utf8') +
    '\nexport const __model = { computeScale, findShift, scrollIn, drawSettled, stopAnimations };';
  const compiled = ts.transpileModule(source, {
    fileName: 'sparkline.tsx',
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      jsx: ts.JsxEmit.ReactJSX,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
    reportDiagnostics: true,
  });
  assert.deepEqual((compiled.diagnostics || []).filter(
    (entry) => entry.category === ts.DiagnosticCategory.Error,
  ), []);

  const d3 = {
    mean: (values) => values.length ? values.reduce((a, b) => a + b, 0) / values.length : undefined,
    max: (values) => values.length ? Math.max(...values) : undefined,
    deviation(values) {
      if (values.length < 2) return undefined;
      const mean = this.mean(values);
      return Math.sqrt(values.reduce((sum, n) => sum + (n - mean) ** 2, 0) / (values.length - 1));
    },
    scaleLinear() {
      const fn = () => 0;
      fn.domain = () => fn;
      fn.range = () => fn;
      return fn;
    },
    curveCatmullRom: { alpha: () => ({}) },
  };
  const pathBuilder = () => {
    const fn = () => 'M0,0';
    for (const key of ['x', 'y', 'y0', 'y1', 'curve']) fn[key] = () => fn;
    return fn;
  };
  d3.line = pathBuilder;
  d3.area = pathBuilder;

  const mocked = {
    d3,
    'es-toolkit': { isEqual: (a, b) => JSON.stringify(a) === JSON.stringify(b) },
    react: { useRef: () => ({ current: null }), useEffect: () => {} },
    '@chimera/utils': { cn: (...values) => values.join(' ') },
    'react/jsx-runtime': { jsx: () => ({}), jsxs: () => ({}) },
  };
  const module = { exports: {} };
  // Evaluate only repository source under a closed set of stubs.
  const evaluate = new Function('require', 'module', 'exports', compiled.outputText);
  evaluate((name) => {
    assert.ok(Object.hasOwn(mocked, name), 'unmocked dependency ' + name);
    return mocked[name];
  }, module, module.exports);
  return module.exports.__model;
}

function chart(data, initial) {
  const animations = [];
  const attributes = new Map();
  function node(name) {
    return {
      style: {},
      animate(frames, options) {
        const animation = {
          frames,
          options,
          effect: { getComputedTiming: () => ({ progress: 0.5 }) },
          cancel() { this.cancelled = true; },
          cancelled: false,
          onfinish: null,
        };
        animations.push({ name, animation });
        return animation;
      },
      setAttribute(key, value) { attributes.set(name + ':' + key, value); },
      removeAttribute(key) { attributes.delete(name + ':' + key); },
    };
  }
  return {
    animations,
    attributes,
    state: {
      slide: node('slide'),
      stretch: node('stretch'),
      area: node('area'),
      line: node('line'),
      size: { width: 300, height: 100 },
      data,
      buf: initial,
      leftGuard: null,
      scale: { yMax: 10, topFactor: 2 / 3 },
      slideAnim: null,
      stretchAnim: null,
    },
  };
}

test('findShift detects actual append-only windows, never unrelated samples', () => {
  const { findShift } = loadModel();
  assert.equal(findShift([1, 2, 3, 4], [2, 3, 4, 5]), 1);
  assert.equal(findShift([1, 2, 3, 4], [3, 4, 5, 6]), 2);
  assert.equal(findShift([1, 2, 3, 4], [10, 11, 12, 13]), 0);
  assert.equal(findShift([1, 2, 3], [2, 3, 4, 5]), 0);
});

test('scale remains finite for all zeros and respects stable versus active variation', () => {
  const { computeScale } = loadModel();
  assert.deepEqual(computeScale([0, 0, 0]), { yMax: 1, topFactor: 2 / 3 });
  assert.deepEqual(computeScale([100, 101, 99, 100]), { yMax: 101, topFactor: 2 / 3 });
  assert.deepEqual(computeScale([0, 200, 50, 10]), { yMax: 200, topFactor: 0.35 });
});

test('new samples schedule compositor translation and settle without per-frame writes', () => {
  const { scrollIn } = loadModel();
  const { state, animations, attributes } = chart([2, 3, 4, 5], [1, 2, 3, 4]);
  scrollIn(state, state.data, 1, 400);
  assert.equal(animations.filter((entry) => entry.name === 'slide').length, 1);
  const first = state.slideAnim.animation;
  assert.deepEqual(first.options, { duration: 400, easing: 'linear', fill: 'forwards' });
  assert.equal(first.frames.at(-1).transform, 'translateX(-100px)');
  assert.equal(attributes.has('area:d'), true);
  assert.equal(attributes.has('line:d'), true);

  state.data = [3, 4, 5, 6];
  scrollIn(state, state.data, 1, 400);
  assert.equal(first.cancelled, true);
  const second = state.slideAnim.animation;
  assert.equal(second.frames[0].transform, 'translateX(-50px)');
  assert.equal(second.frames.at(-1).transform, 'translateX(-200px)');
  second.onfinish();
  assert.equal(state.slideAnim, null);
  assert.deepEqual(state.buf, [3, 4, 5, 6]);
});

test('a first peak does not stretch the chart from zero and animations clean up', () => {
  const { scrollIn, drawSettled } = loadModel();
  const { state, animations } = chart([0, 0, 1000], [0, 0, 0]);
  state.scale = { yMax: 1, topFactor: 2 / 3 };
  scrollIn(state, state.data, 1, 250);
  assert.equal(animations.filter((entry) => entry.name === 'stretch').length, 0);
  assert.equal(animations.filter((entry) => entry.name === 'slide').length, 1);
  drawSettled(state);
  assert.equal(state.slideAnim, null);
  assert.equal(state.stretchAnim, null);
  assert.deepEqual(state.buf, state.data);

  state.data = [5];
  drawSettled(state);
  assert.equal(state.buf, null);
});
