/**
 * Server-side ScrollArea contract tests using only the installed React/Vite
 * dependencies. Real scroll direction, ref-attachment timing and sticky header
 * behavior must also be checked with desktop E2E; SSR cannot prove these.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

async function withScrollArea(run: (api: Record<string, any>) => void) {
  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    server: { middlewareMode: true, ws: false },
    oxc: { jsx: { runtime: 'automatic' } },
    appType: 'custom',
    logLevel: 'error',
  });
  try {
    const api = await vite.ssrLoadModule('/src/scroll-area.tsx');
    run(api);
  } finally {
    await vite.close();
  }
}

test('both scroll containers expose the expected slots and initial state', async () => {
  await withScrollArea(({ ScrollArea, AppContentScrollArea }) => {
    for (const [Component, slot] of [
      [ScrollArea, 'scroll-area'],
      [AppContentScrollArea, 'app-content-scroll-area'],
    ]) {
      const html = renderToStaticMarkup(
        h(
          Component,
          { 'aria-label': 'Activity', scrollbars: 'both' },
          h('p', null, 'Items'),
        ),
      );
      assert.match(html, new RegExp('data-slot="' + slot + '"'));
      assert.match(html, /data-slot="scroll-area-viewport"/);
      assert.match(html, /data-top="true"/);
      assert.match(html, /data-scroll-direction="none"/);
      assert.match(html, /Items/);
    }
  });
});

test('scroll hooks reject use outside their owning ScrollArea', async () => {
  await withScrollArea(
    ({
      useScrollArea,
      useScrollAreaViewport,
      useScrollAreaViewportElement,
    }) => {
      for (const [hook, message] of [
        [useScrollArea, /useScrollArea must be used within a ScrollArea/],
        [
          useScrollAreaViewport,
          /useScrollAreaViewport must be used within a ScrollArea/,
        ],
        [
          useScrollAreaViewportElement,
          /useScrollAreaViewportElement must be used within a ScrollArea/,
        ],
      ] as const) {
        assert.throws(
          () =>
            renderToStaticMarkup(
              h(() => {
                hook();
                return null;
              }),
            ),
          message,
        );
      }
    },
  );
});

test('scroll context initializes top and header flags for consumers', async () => {
  await withScrollArea(
    ({
      ScrollArea,
      AppContentScrollArea,
      useScrollArea,
      useScrollAreaViewport,
    }) => {
      for (const Component of [ScrollArea, AppContentScrollArea]) {
        function Consumer() {
          const state = useScrollArea();
          const { viewportRef } = useScrollAreaViewport();
          assert.deepEqual(
            {
              isTop: state.isTop,
              isBottom: state.isBottom,
              isScrolling: state.isScrolling,
              isPastHeader: state.isPastHeader,
              direction: state.scrollDirection,
            },
            {
              isTop: true,
              isBottom: false,
              isScrolling: false,
              isPastHeader: false,
              direction: 'none',
            },
          );
          assert.ok(viewportRef);
          return h('span', { 'data-slot': 'scroll-state-contract' }, 'ready');
        }
        const html = renderToStaticMarkup(h(Component, null, h(Consumer)));
        assert.match(html, /scroll-state-contract/);
      }
    },
  );
});
