/**
 * Node-side component contracts loaded through Vite, with virtual icon components
 * stubbed at the presentation boundary. These assertions exercise real UI source,
 * Radix semantics, and HCT parsing, but not browser focus/keyboard or animation.
 */
import assert from 'node:assert/strict';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { createElement as h } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

test(
  'ref menu/navigation/color primitives preserve semantic markup and HCT input rules',
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
      ssr: { noExternal: ['@material/material-color-utilities'] },
      server: { middlewareMode: true, ws: false },
      appType: 'custom',
      logLevel: 'error',
    });

    try {
      const { MenuSurface } = await vite.ssrLoadModule('/src/menu-surface.tsx');
      const { SearchField } = await vite.ssrLoadModule(
        '/src/search-field.tsx',
      );
      const { Menubar, MenubarMenu, MenubarTrigger } =
        await vite.ssrLoadModule('/src/menubar.tsx');
      const {
        NavigationList,
        NavigationListGroup,
        NavigationListItem,
        NavigationListItemText,
      } = await vite.ssrLoadModule('/src/navigation-list.tsx');
      const { Select, SelectTrigger, SelectValue } =
        await vite.ssrLoadModule('/src/select.tsx');
      const { FabMenu, FabMenuTrigger } =
        await vite.ssrLoadModule('/src/fab-menu.tsx');
      const { parseHexColor } = await vite.ssrLoadModule(
        '/src/hct-color-picker.tsx',
      );

      const surface = renderToStaticMarkup(h(MenuSurface, null, 'Options'));
      assert.match(surface, /Options/);
      assert.match(surface, /rounded/);

      // SSR exercises the production SearchField structure, not keyboard focus.
      // An editable nonempty search retains a visible clear action.
      const basicSearch = renderToStaticMarkup(
        h(SearchField, {
          value: 'widgets',
          onValueChange: () => {},
          clearLabel: 'Clear search',
          placeholder: 'Search widgets',
        }),
      );
      assert.match(basicSearch, /data-slot="search-field"/);
      assert.match(basicSearch, /type="search"/);
      assert.match(basicSearch, /value="widgets"/);
      assert.match(basicSearch, /aria-label="Search widgets"/);
      assert.match(basicSearch, /data-slot="search-field-clear"/);
      assert.match(basicSearch, /aria-label="Clear search"/);
      assert.doesNotMatch(basicSearch, /shadow-lg/);

      const glassSearch = renderToStaticMarkup(
        h(SearchField, {
          value: 'view',
          onValueChange: () => {},
          clearLabel: 'Clear',
          variant: 'glass',
        }),
      );
      assert.match(glassSearch, /shadow-lg/);
      assert.match(glassSearch, /h-11/);

      for (const inputProp of ['disabled', 'readOnly'] as const) {
        const lockedSearch = renderToStaticMarkup(
          h(SearchField, {
            value: 'important',
            onValueChange: () => {},
            clearLabel: 'Clear',
            [inputProp]: true,
          }),
        );
        assert.doesNotMatch(lockedSearch, /data-slot="search-field-clear"/);
      }

      const labelledSearch = renderToStaticMarkup(
        h(SearchField, {
          value: '',
          onValueChange: () => {},
          clearLabel: 'Clear',
          placeholder: 'Search widgets',
          'aria-labelledby': 'search-caption',
        }),
      );
      assert.match(labelledSearch, /aria-labelledby="search-caption"/);
      assert.doesNotMatch(labelledSearch, /aria-label="Search widgets"/);

      const menubar = renderToStaticMarkup(
        h(Menubar, null, h(MenubarMenu, null, h(MenubarTrigger, null, 'File'))),
      );
      assert.match(menubar, /role="menubar"/);
      assert.match(menubar, /data-slot="menubar-trigger"/);
      assert.match(menubar, /role="menuitem"/);

      const navigation = renderToStaticMarkup(
        h(
          NavigationList,
          { 'aria-label': 'Folders' },
          h(
            NavigationListGroup,
            { label: 'Folders' },
            h(
              NavigationListItem,
              { active: true, 'data-dragging': false },
              h(NavigationListItemText, {
                headline: 'Work',
                supporting: '3 profiles',
              }),
            ),
          ),
        ),
      );
      assert.match(navigation, /data-slot="navigation-list"/);
      assert.match(navigation, /role="group"/);
      assert.match(navigation, /data-active="true"/);
      assert.match(navigation, /data-dragging="false"/);
      assert.match(navigation, /Work/);

      const select = renderToStaticMarkup(
        h(
          Select,
          { defaultValue: 'auto' },
          h(SelectTrigger, null, h(SelectValue, { placeholder: 'Choose' })),
        ),
      );
      assert.match(select, /role="combobox"/);

      const fab = renderToStaticMarkup(
        h(FabMenu, null, h(FabMenuTrigger, { icon: h('span', null, '+') })),
      );
      assert.match(fab, /data-slot="fab-menu-trigger"/);
      assert.match(fab, /aria-haspopup="menu"/);

      for (const [input, expected] of [
        [' #AbC ', '#aabbcc'],
        ['ffa011', '#ffa011'],
        ['#fff', '#ffffff'],
        ['#12345678', null],
        ['#abcd', null],
        ['#fffggg', null],
        ['', null],
      ] as const) {
        assert.equal(
          parseHexColor(input),
          expected,
          'Unexpected HCT hex normalization for ' + JSON.stringify(input),
        );
      }
    } finally {
      await vite.close();
    }
  },
);
