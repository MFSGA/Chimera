/**
 * Fixture-level SSR contracts for the shared primitives. No browser, animation,
 * WebDriver, Tauri IPC, or host state is exercised here.
 *
 * The repository's UI tsconfig uses the classic JSX transform under tsx;
 * its normal Vite application build supplies the JSX runtime. The test adapts
 * that fixture environment only, without changing product behavior.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { LinearProgress as LegacyLinearProgress } from '../src/linear-progress';
import { CircularProgress, LinearProgress } from '../src/progress';
import { Switch, SwitchItem } from '../src/switch';

(globalThis as typeof globalThis & { React: typeof React }).React = React;

const render = (element: React.ReactElement) => renderToStaticMarkup(element);

test('reference linear progress clamps values and exposes determinate state', () => {
  const over = render(React.createElement(LinearProgress, { value: 140 }));
  assert.match(over, /aria-valuenow="100"/);
  assert.match(over, /data-slot="linear-progress-indicator"/);
  assert.match(over, /width:100%/);

  const under = render(React.createElement(LinearProgress, { value: -8 }));
  assert.match(under, /aria-valuenow="0"/);
  assert.match(under, /width:0%/);
});

test('reference linear progress omits a numeric value and shows both animations while indeterminate', () => {
  const markup = render(
    React.createElement(LinearProgress, { indeterminate: true, value: 47 }),
  );
  assert.doesNotMatch(markup, /aria-valuenow=/);
  assert.match(markup, /data-slot="linear-progress-indeterminate-primary"/);
  assert.match(markup, /data-slot="linear-progress-indeterminate-secondary"/);
  assert.doesNotMatch(markup, /data-slot="linear-progress-indicator"/);
});

test('legacy linear-progress direct-import path still renders its own established bar contract', () => {
  const markup = render(
    React.createElement(LegacyLinearProgress, { value: 28 }),
  );
  assert.match(markup, /data-slot="linear-progress-bar"/);
  assert.match(markup, /aria-valuenow="28"/);
  assert.doesNotMatch(markup, /data-slot="linear-progress-indicator"/);
});

test('circular progress preserves distinct determinate and indeterminate slots', () => {
  const determinate = render(
    React.createElement(CircularProgress, { value: 45 }),
  );
  assert.match(determinate, /data-slot="circular-progress-determinate"/);
  assert.doesNotMatch(
    determinate,
    /data-slot="circular-progress-indeterminate"/,
  );

  const indeterminate = render(
    React.createElement(CircularProgress, { indeterminate: true }),
  );
  assert.match(indeterminate, /data-slot="circular-progress-indeterminate"/);
  assert.match(
    indeterminate,
    /data-slot="circular-progress-indeterminate-left"/,
  );
  assert.match(
    indeterminate,
    /data-slot="circular-progress-indeterminate-right"/,
  );
  assert.doesNotMatch(
    indeterminate,
    /data-slot="circular-progress-determinate"/,
  );
});

test('reference loading switch exposes the spinner slots only while loading', () => {
  const loading = render(
    React.createElement(Switch, { loading: true, defaultChecked: true }),
  );
  assert.match(loading, /data-slot="switch-thumb-loading"/);
  assert.match(loading, /data-slot="switch-thumb-loading-circular-progress"/);

  const idle = render(React.createElement(Switch, { loading: false }));
  assert.doesNotMatch(idle, /data-slot="switch-thumb-loading"/);
});

test('SwitchItem keeps its children and renders a switch', () => {
  const markup = render(
    React.createElement(SwitchItem, { 'aria-label': 'toggle' }, 'Preference'),
  );
  assert.match(markup, /Preference/);
  assert.match(markup, /aria-label="toggle"/);
  assert.match(markup, /role="switch"/);
});
