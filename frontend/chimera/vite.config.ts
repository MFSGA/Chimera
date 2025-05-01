import { defineConfig, UserConfig } from "vite";
import react from "@vitejs/plugin-react";

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

// https://vitejs.dev/config/
export default defineConfig(async ({ command }) => {
  const isDev = command === 'serve'

  const config = {
    root: "src",
    server: { port: 3000 },

    plugins: [react(), isDev && devtools(),],

    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent vite from obscuring rust errors
    clearScreen: false,
  } satisfies UserConfig
  return config
});
