// features
// langs
import 'monaco-editor/languages/definitions/javascript/register';
import 'monaco-editor/languages/definitions/lua/register';
import 'monaco-editor/languages/definitions/yaml/register';
import 'monaco-editor/features/register.all';
import 'monaco-editor/features/links/register';
// language services
import * as monaco from 'monaco-editor';
import 'monaco-editor/languages/features/typescript/register';
// others
import { loader } from '@monaco-editor/react';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';
import cssWorker from 'monaco-editor/language/css/css.worker?worker';
import jsonWorker from 'monaco-editor/language/json/json.worker?worker';
import tsWorker from 'monaco-editor/language/typescript/ts.worker?worker';
// workers
import yamlWorker from '@/utils/monaco-yaml.worker?worker';

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

// monaco-yaml 5.5.1 uses monaco-worker-manager 2.0.1, which still calls
// createWebWorker({ label, moduleId, createData }). Monaco 0.57 requires
// a Worker instead. Adapt only the YAML plugin, without mutating the global
// editor API. The two initialization messages follow Monaco's own
// internal/common/workers.js compatibility implementation.
type LegacyYamlWorkerOptions = {
  label: 'yaml';
  moduleId: string;
  createData: unknown;
};

export const yamlMonaco = {
  ...monaco,
  editor: {
    ...monaco.editor,
    createWebWorker<T extends object>(
      options:
        monaco.editor.IInternalWebWorkerOptions | LegacyYamlWorkerOptions,
    ): monaco.editor.MonacoWebWorker<T> {
      if ('worker' in options) {
        return monaco.editor.createWebWorker<T>(options);
      }

      const worker = Promise.resolve().then(() => {
        const instance = new yamlWorker();
        instance.postMessage('ignore');
        instance.postMessage(options.createData);
        return instance;
      });
      return monaco.editor.createWebWorker<T>({ worker });
    },
  },
};

loader.config({ monaco });

loader
  .init()
  .then(() => {
    console.log('Monaco is ready');
  })
  .catch((error) => {
    console.error('Monaco initialization failed', error);
  });
