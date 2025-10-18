import path from 'node:path';
import { TanStackRouterVite } from '@tanstack/router-plugin/vite';
import react from '@vitejs/plugin-react';
import { NodePackageImporter } from 'sass-embedded';
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
            title: 'Clash Nyanpasu',
            injectScript:
              mode === 'development'
                ? '<script src="https://unpkg.com/react-scan/dist/auto.global.js"></script>'
                : '',
          },
        },
      }),
      react({}),
      sassDts({ esmExport: true }),
      isDev && devtools(),
    ],

    resolve: {
      alias: {
        // todo: will deleete
        '@': path.resolve(__dirname, './src'),
      },
    },

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent vite from obscuring rust errors
    clearScreen: false,

    build: {
      outDir: '../../backend/tauri/tmp/dist',
      rollupOptions: {
        output: {
          /*  manualChunks: {
             jsonWorker: [`monaco-editor/esm/vs/language/json/json.worker`],
             tsWorker: [`monaco-editor/esm/vs/language/typescript/ts.worker`],
             editorWorker: [`monaco-editor/esm/vs/editor/editor.worker`],
             yamlWorker: [`monaco-yaml/yaml.worker`],
           }, */
        },
      },
      emptyOutDir: true,
      sourcemap: isDev || IS_NIGHTLY ? 'inline' : false,
    },
  } satisfies UserConfig;
  return config;
});
