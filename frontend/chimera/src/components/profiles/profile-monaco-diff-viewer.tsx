import '@/services/monaco';
import * as monaco from 'monaco-editor';
import { useLayoutEffect, useRef } from 'react';
import { beforeEditorMount } from './profile-monaco-viewer';

type ProfileMonacoDiffViewerProps = {
  original?: string;
  modified?: string;
  originalModelPath: string;
  modifiedModelPath: string;
  language: string;
  theme: string;
  options?: monaco.editor.IDiffEditorOptions;
};

// Unlike the generic React DiffEditor wrapper, this component exclusively
// owns both models. Detach them before disposing the widget or its models.
export default function ProfileMonacoDiffViewer({
  original = '',
  modified = '',
  originalModelPath,
  modifiedModelPath,
  language,
  theme,
  options,
}: ProfileMonacoDiffViewerProps) {
  const container = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneDiffEditor | null>(null);

  useLayoutEffect(() => {
    if (!container.current) return;

    beforeEditorMount();
    const originalUri = monaco.Uri.parse(originalModelPath);
    const modifiedUri = monaco.Uri.parse(modifiedModelPath);
    const originalModel = monaco.editor.createModel('', language, originalUri);
    let modifiedModel: monaco.editor.ITextModel | undefined;
    let widget: monaco.editor.IStandaloneDiffEditor | undefined;

    const dispose = () => {
      editor.current = null;
      widget?.setModel(null);
      widget?.dispose();
      originalModel.dispose();
      modifiedModel?.dispose();
    };

    try {
      modifiedModel = monaco.editor.createModel('', language, modifiedUri);
      widget = monaco.editor.createDiffEditor(container.current, {
        automaticLayout: true,
        readOnly: true,
      });
      widget.setModel({ original: originalModel, modified: modifiedModel });
      editor.current = widget;
    } catch (error) {
      dispose();
      throw error;
    }

    return dispose;
  }, [language, originalModelPath, modifiedModelPath]);

  useLayoutEffect(() => {
    const model = editor.current?.getModel();
    if (model?.original.getValue() !== original) {
      model?.original.setValue(original);
    }
    if (model?.modified.getValue() !== modified) {
      model?.modified.setValue(modified);
    }
  }, [original, modified, language, originalModelPath, modifiedModelPath]);

  useLayoutEffect(() => {
    editor.current?.updateOptions(options ?? {});
    monaco.editor.setTheme(theme);
  }, [options, theme, language, originalModelPath, modifiedModelPath]);

  return <div ref={container} className="h-full w-full min-w-0" />;
}
