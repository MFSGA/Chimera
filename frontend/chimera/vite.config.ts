import path from 'node:path';
import { TanStackRouterVite } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { NodePackageImporter } from 'sass-embedded';
import Icons from 'unplugin-icons/vite';
import { defineConfig, UserConfig } from 'vite';
import { createHtmlPlugin } from 'vite-plugin-html';
import sassDts from 'vite-plugin-sass-dts';

const host = process.env.TAURI_DEV_HOST;

const devtools = () => {
  return {
    name: 'react-devtools',
    transformIndexHtml(html: string) {
      return html.replace(
        /<\/head>/,
        `<script src="http://localhost:8097"></script></head>`,
      );
    },
  };
};

const IS_NIGHTLY = process.env.NIGHTLY?.toLowerCase() === 'true';

// https://vitejs.dev/config/
export default defineConfig(async ({ command, mode }) => {
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
      TanStackRouterVite(),
      createHtmlPlugin({
        inject: {
          data: {
            title: 'Clash Chimera',
            injectScript:
              mode === 'development'
                ? '<script src="https://unpkg.com/react-scan/dist/auto.global.js"></script>'
                : '',
          },
        },
      }),
      react({}),
      Icons({ compiler: 'jsx' }),
      sassDts({ esmExport: true }),
      isDev && devtools(),
    ],

    resolve: {
      alias: [
        { find: '@root', replacement: path.resolve(__dirname, '../..') },
        { find: '@', replacement: path.resolve(__dirname, './src') },
        { find: '~', replacement: path.resolve(__dirname, '.') },
      ],
      dedupe: ['react', 'react-dom'],
    },

    optimizeDeps: {
      entries: ['./src/main.tsx'],
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
  } satisfies UserConfig;
  return config;
});
