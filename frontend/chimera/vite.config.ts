import path from 'node:path';
import { paraglideVitePlugin } from '@inlang/paraglide-js';
import tailwindPlugin from '@tailwindcss/vite';
import { tanstackRouter } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { NodePackageImporter } from 'sass-embedded';
import Icons from 'unplugin-icons/vite';
import { defineConfig, UserConfig } from 'vite';
import { createHtmlPlugin } from 'vite-plugin-html';
import sassDts from 'vite-plugin-sass-dts';

const IS_NIGHTLY = process.env.NIGHTLY?.toLowerCase() === 'true';

// https://vitejs.dev/config/
export default defineConfig(async ({ command }) => {
  const isDev = command === 'serve';

  const config = {
    // root: "src",
    server: { port: 3000 },

    css: {
      preprocessorOptions: {
        scss: {
          api: 'modern-compiler',
          importer: [new NodePackageImporter()],
        },
      },
    },
    plugins: [
      tailwindPlugin(),
      tanstackRouter({
        target: 'react',
        autoCodeSplitting: true,
        routesDirectory: 'src/pages',
        generatedRouteTree: 'src/routeTree.gen.ts',
        routeFileIgnorePattern: '_modules',
      }),
      createHtmlPlugin({
        inject: {
          data: {
            title: 'Clash Chimera',
            injectScript: '',
          },
        },
      }),
      react({}),
      Icons({ compiler: 'jsx' }),
      sassDts({ esmExport: true, legacyFileFormat: true }),
      paraglideVitePlugin({
        project: './project.inlang',
        outdir: './src/paraglide',
        // The cache strategy is unset on first launch, so baseLocale must end
        // the chain until LanguageProvider applies the backend's locale.
        strategy: ['custom-extension', 'baseLocale'],
      }),
    ],

    resolve: {
      alias: [
        { find: '@root', replacement: path.resolve(__dirname, '../..') },
        { find: '@', replacement: path.resolve(__dirname, './src') },
        { find: '~', replacement: path.resolve(__dirname, '.') },
        { find: '@chimera/utils', replacement: path.resolve('../utils/src') },
        { find: '@interface', replacement: path.resolve('../interface/src') },
        {
          // monaco-worker-manager (monaco-yaml dep) still imports the pre-0.56
          // deep `monaco-editor/esm/vs/*` paths; rewrite them to specifiers the
          // new exports map resolves to the same files.
          find: /^monaco-editor\/esm\/vs\//,
          replacement: 'monaco-editor/',
        },
      ],
      dedupe: ['react', 'react-dom', '@tanstack/react-query'],
    },

    optimizeDeps: {
      entries: ['./src/main.tsx'],
      // clsx is owned by @chimera/ui, not this application package.
      include: ['@tauri-apps/api', 'react', 'react-dom'],
    },

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent vite from obscuring rust errors
    clearScreen: false,

    worker: {
      format: 'es',
      rolldownOptions: {
        output: {
          manualChunks: (id) => {
            if (id.includes('monaco-editor/esm/vs/language/css/css.worker')) {
              return 'css-worker';
            }

            if (id.includes('monaco-editor/esm/vs/language/json/json.worker')) {
              return 'json-worker';
            }

            if (
              id.includes('monaco-editor/esm/vs/language/typescript/ts.worker')
            ) {
              return 'ts-worker';
            }

            if (id.includes('monaco-editor/esm/vs/editor/editor.worker')) {
              return 'editor-worker';
            }

            if (id.includes('monaco-yaml/yaml.worker')) {
              return 'yaml-worker';
            }
          },
        },
      },
    },

    build: {
      outDir: '../../backend/tauri/tmp/dist',
      emptyOutDir: true,
      sourcemap: isDev || IS_NIGHTLY ? 'inline' : false,
    },
    define: {
      OS_PLATFORM: `"${process.platform}"`,
      WIN_PORTABLE: !!process.env.VITE_WIN_PORTABLE,
      IS_NIGHTLY,
    },
  } satisfies UserConfig;
  return config;
});
