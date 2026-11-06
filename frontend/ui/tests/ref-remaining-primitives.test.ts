/**
 * SSR fixtures exercise browser-independent portions of the shared UI source
 * with virtual icons stubbed. SharedTooltip touches document during render,
 * so this test checks its module export but leaves behavior to browser E2E.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

test(
  'DateTimeField, tooltip, glass controls and navigation sidebar preserve contracts',
  { timeout: 30_000 },
  async () => {
    const vite = await createServer({
      configFile: false,
      root: fileURLToPath(new URL('../', import.meta.url)),
      plugins: [
        {
          name: 'ref-ui-test-icons',
          enforce: 'pre',
          resolveId(id) {
            if (id.startsWith('~icons/')) {
              return '\0ref-ui-test-icon:' + id;
            }
          },
          load(id) {
            if (id.startsWith('\0ref-ui-test-icon:')) {
              return 'import { createElement } from "react"; export default function Icon() { return createElement("svg", { "aria-hidden": true }); }';
            }
          },
        },
      ],
      esbuild: { jsx: 'automatic' },
      server: { middlewareMode: true, ws: false },
      appType: 'custom',
      logLevel: 'error',
    });

    try {
      const { default: DateTimeField } = await vite.ssrLoadModule(
        '/src/date-time-field.tsx',
      );
      const { SharedTooltip } = await vite.ssrLoadModule(
        '/src/shared-tooltip.tsx',
      );
      const { TooltipBubble } = await vite.ssrLoadModule('/src/tooltip.tsx');
      const { Button, buttonVariants } =
        await vite.ssrLoadModule('/src/button.tsx');
      const { GlassControlGroup } = await vite.ssrLoadModule(
        '/src/glass-control-group.tsx',
      );
      const { segmentedButtonVariants, segmentedButtonItemVariants } =
        await vite.ssrLoadModule('/src/segmented-button.tsx');
      const { SidebarProvider } = await vite.ssrLoadModule(
        '/src/slider-sidebar.tsx',
      );
      const { NavigationSidebar, NAVIGATION_SIDEBAR_WIDTH } =
        await vite.ssrLoadModule('/src/navigation-sidebar.tsx');

      const date = renderToStaticMarkup(
        h(DateTimeField, {
          label: 'Update time',
          locale: 'en-US',
          value: '2026-11-04T19:30',
          openCalendarLabel: 'Open calendar',
          clearLabel: 'Clear',
          onChange: () => {},
        }),
      );
      assert.match(date, /Update time/);
      assert.match(date, /Open calendar/);
      assert.match(date, /Clear/);
      assert.match(date, /19/);
      assert.match(date, /30/);

      assert.equal(typeof SharedTooltip, 'function');

      const bubble = renderToStaticMarkup(
        h(TooltipBubble, { side: 'top' }, 'Quick tip'),
      );
      assert.match(bubble, /Quick tip/);
      assert.match(bubble, /rounded-full/);

      assert.match(buttonVariants({ variant: 'glass' }), /shadow-lg/);
      assert.match(
        segmentedButtonVariants({ variant: 'glass' }),
        /segmented-button-slider-pill/,
      );
      assert.match(
        segmentedButtonItemVariants({ variant: 'glass' }),
        /whitespace-nowrap/,
      );

      const glassGroup = renderToStaticMarkup(
        h(
          GlassControlGroup,
          { 'aria-label': 'Quick actions' },
          h(Button, { variant: 'glass', disabled: true }, 'Search'),
        ),
      );
      assert.match(glassGroup, /data-slot="glass-control-group"/);
      assert.match(glassGroup, /aria-label="Quick actions"/);
      assert.match(glassGroup, /data-loading="false"/);
      assert.match(glassGroup, /Search/);

      assert.deepEqual(NAVIGATION_SIDEBAR_WIDTH, { open: 200, closed: 88 });
      const navigation = renderToStaticMarkup(
        h(
          SidebarProvider,
          { defaultOpen: true },
          h(
            NavigationSidebar,
            { 'aria-label': 'Sections' },
            h('span', null, 'Workspace'),
          ),
        ),
      );
      assert.match(navigation, /data-slot="slider-sidebar"/);
      assert.match(navigation, /data-slot="navigation-sidebar"/);
      assert.match(navigation, /aria-label="Sections"/);
      assert.match(navigation, /Workspace/);
    } finally {
      await vite.close();
    }
  },
);
