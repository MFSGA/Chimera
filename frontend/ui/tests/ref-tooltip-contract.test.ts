/**
 * Node SSR contract checks. Not browser focus/hover or desktop E2E.
 */
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

test('reference Tooltip renders beneath a shared application provider', async () => {
  const vite = await createServer({
    configFile: false,
    root: fileURLToPath(new URL('../', import.meta.url)),
    oxc: { jsx: { runtime: 'automatic' } },
    server: { middlewareMode: true, ws: false },
    appType: 'custom',
    logLevel: 'error',
  });
  try {
    const {
      TooltipProvider,
      Tooltip,
      TooltipTrigger,
      TooltipContent,
      TooltipBubble,
    } = await vite.ssrLoadModule('/src/tooltip.tsx');
    const closed = renderToStaticMarkup(
      h(
        TooltipProvider,
        null,
        h(
          Tooltip,
          { open: false },
          h(TooltipTrigger, null, 'Help'),
          h(TooltipContent, null, 'Tooltip body'),
        ),
      ),
    );
    assert.match(closed, /Help/);
    assert.doesNotMatch(closed, /Tooltip body/);
    const bubble = renderToStaticMarkup(
      h(TooltipBubble, { side: 'top' }, 'Bubble content'),
    );
    assert.match(bubble, /Bubble content/);
    assert.match(bubble, /rounded-full/);
  } finally {
    await vite.close();
  }
});

test('router Outlet uses one top-level TooltipProvider', () => {
  const root = readFileSync(
    new URL('../../chimera/src/pages/__root.tsx', import.meta.url),
    'utf8',
  );
  assert.match(
    root,
    /import \{ TooltipProvider \} from '@chimera\/ui\/tooltip'/,
  );
  assert.match(root, /<TooltipProvider>\s*<Outlet \/>\s*<\/TooltipProvider>/);
});
