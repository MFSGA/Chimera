import { defineConfig, UserConfig } from "vite";
import react from "@vitejs/plugin-react";
import { TanStackRouterVite } from "@tanstack/router-plugin/vite";
import path from 'node:path'


const host = process.env.TAURI_DEV_HOST;

const devtools = () => {
  return {
    name: 'react-devtools',
    transformIndexHtml(html: string) {
      return html.replace(
        /<\/head>/,
        `<script src="http://localhost:8097"></script></head>`,
      )
    },
  }
}

const IS_NIGHTLY = process.env.NIGHTLY?.toLowerCase() === 'true'

// https://vitejs.dev/config/
export default defineConfig(async ({ command }) => {
  const isDev = command === 'serve'

  const config = {
    // root: "src",
    server: { port: 3000 },

    plugins: [TanStackRouterVite(), react({}), isDev && devtools(),
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
  } satisfies UserConfig
  return config
});
