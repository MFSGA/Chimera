/**
 * SSR fixture smoke tests for reference visual primitives. These verify
 * declarative markup only, not transitions, loading completion, or pointer input.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import * as React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { ActionSwapText } from '../src/action-swap-text';
import { Kbd } from '../src/kbd';
import {
  LauncherMenu,
  LauncherMenuChipButton,
  LauncherMenuGroup,
  LauncherMenuIconButton,
  LauncherMenuRow,
} from '../src/launcher-menu';
import { LazyImage } from '../src/lazy-image';
import { MaterialShape } from '../src/material-shape';

// This fixture uses tsx with classic JSX, unlike Vite's application JSX runtime.
(globalThis as typeof globalThis & { React: typeof React }).React = React;

const render = (element: React.ReactElement) => renderToStaticMarkup(element);

test('ActionSwapText exposes content in the live key and does not show absent text', () => {
  const visible = render(
    React.createElement(ActionSwapText, { value: 'Active' }),
  );
  assert.match(visible, /data-slot="action-swap-text"/);
  assert.match(visible, /Active/);
  const empty = render(React.createElement(ActionSwapText, { value: null }));
  assert.doesNotMatch(empty, />Active</);
});

test('Kbd uses a semantic kbd element and forwards children', () => {
  const markup = render(React.createElement(Kbd, null, '⌘'));
  assert.match(markup, /^<kbd\b/);
  assert.match(markup, /⌘/);
});

test('launcher group preserves role and buttons have safe default type', () => {
  const markup = render(
    React.createElement(
      LauncherMenu,
      null,
      React.createElement(
        LauncherMenuGroup,
        null,
        React.createElement(LauncherMenuRow, null, 'Network'),
        React.createElement(LauncherMenuIconButton, null, '?'),
        React.createElement(LauncherMenuChipButton, null, 'Auto'),
      ),
    ),
  );
  assert.match(markup, /data-slot="launcher-menu-group"/);
  assert.match(markup, /role="group"/);
  assert.match(markup, /data-slot="launcher-menu-row"/);
  assert.match(markup, /data-slot="launcher-menu-icon-button"/);
  assert.match(markup, /data-slot="launcher-menu-chip-button"/);
  assert.equal((markup.match(/type="button"/g) ?? []).length, 2);
});

test('LazyImage starts with a skeleton and hidden image until load', () => {
  const markup = render(
    React.createElement(LazyImage, {
      src: '/placeholder.png',
      alt: 'Placeholder',
      className: 'size-8',
    }),
  );
  assert.match(markup, /animate-pulse/);
  assert.match(markup, /<img\b/);
  assert.match(markup, /alt="Placeholder"/);
  assert.match(markup, /class="size-8 hidden"/);
});

test('MaterialShape renders reference shape slot', () => {
  const markup = render(React.createElement(MaterialShape, { active: true }));
  assert.match(markup, /data-slot="material-shape"/);
});
