/**
 * SSR surface contract for the pinned reference sparkline.
 * Real CSS layout, ResizeObserver and Web Animations require browser E2E.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

test('Sparkline keeps the dashboard prop contract and ref layered markup', async () => {
  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    oxc: { jsx: { runtime: 'automatic' } },
    server: { middlewareMode: true, ws: false },
    appType: 'custom',
    logLevel: 'error',
  });
  try {
    const { Sparkline } = await vite.ssrLoadModule('/src/sparkline.tsx');
    const markup = renderToStaticMarkup(
      h(Sparkline, {
        data: [0, 4, 3, 8],
        animationDuration: 0.4,
        className: 'chart-probe',
        'aria-label': 'Recent traffic',
      }),
    );
    assert.match(markup, /<div[^>]*data-slot="sparkline"/);
    assert.match(markup, /chart-probe/);
    assert.match(markup, /aria-label="Recent traffic"/);
    assert.match(markup, /<svg/);
    assert.match(markup, /class="area fill-primary\/10"/);
    assert.match(markup, /class="line stroke-primary"/);
    assert.doesNotMatch(markup, /<svg[^>]*data-slot="sparkline"/);
  } finally {
    await vite.close();
  }
});
