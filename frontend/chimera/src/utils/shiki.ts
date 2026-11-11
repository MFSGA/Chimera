import type { Highlighter } from 'shiki';
import { getSingletonHighlighterCore } from 'shiki/core';
import { createOnigurumaEngine } from 'shiki/engine/oniguruma';
import minLight from 'shiki/themes/min-light.mjs';
import nord from 'shiki/themes/nord.mjs';
import getWasm from 'shiki/wasm';

let shiki: Highlighter | null = null;

export async function getShikiSingleton() {
  if (!shiki) {
    shiki = (await getSingletonHighlighterCore({
      engine: createOnigurumaEngine(getWasm),
      themes: [nord, minLight],
      langs: [() => import('shiki/langs/shell.mjs')],
    })) as Highlighter;
  }
  return shiki;
}

/** Shared on-demand JSON highlighter for the ref-aligned log viewer. */
export async function highlightJson(code: string) {
  const instance = await getShikiSingleton();
  await instance.loadLanguage(import('shiki/langs/json.mjs'));
  return instance.codeToHtml(code, {
    lang: 'json',
    themes: { dark: 'nord', light: 'min-light' },
  });
}

export async function formatAnsi(str: string) {
  const instance = await getShikiSingleton();
  return instance.codeToHtml(str, {
    lang: 'ansi',
    themes: {
      dark: 'nord',
      light: 'min-light',
    },
  });
}
