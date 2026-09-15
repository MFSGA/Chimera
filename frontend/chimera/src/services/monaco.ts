import { loader } from '@monaco-editor/react';
import type * as Monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';
import cssWorker from 'monaco-editor/language/css/css.worker?worker';
import jsonWorker from 'monaco-editor/language/json/json.worker?worker';
import tsWorker from 'monaco-editor/language/typescript/ts.worker?worker';
import yamlWorker from '@/utils/monaco-yaml.worker?worker';

let initPromise: Promise<typeof Monaco> | undefined;

function configureWorkers() {
  if (self.MonacoEnvironment) {
    return;
  }

  self.MonacoEnvironment = {
    getWorker(_, label) {
      switch (label) {
        case 'json':
          return new jsonWorker();
        case 'typescript':
        case 'javascript':
          return new tsWorker();
        case 'css':
        case 'less':
        case 'scss':
          return new cssWorker();
        case 'yaml':
          return new yamlWorker();
        default:
          return new editorWorker();
      }
    },
  };
}

export function loadMonaco() {
  if (!initPromise) {
    initPromise = import('monaco-editor').then(async (monaco) => {
      configureWorkers();

      await import('monaco-editor/languages/definitions/javascript/register');
      await import('monaco-editor/languages/definitions/lua/register');
      await import('monaco-editor/languages/definitions/yaml/register');
      await import('monaco-editor/features/register.all');
      await import('monaco-editor/features/links/register');
      await import('monaco-editor/languages/features/typescript/register');

      loader.config({ monaco });
      return monaco;
    });
  }

  return initPromise;
}
