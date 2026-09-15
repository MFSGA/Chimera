import { DiffEditor, DiffEditorProps } from '@monaco-editor/react';
import { loadMonaco } from '@/services/monaco';
import { beforeEditorMount } from './profile-monaco-viewer';

export default function ProfileMonacoDiffViewer(
  props: Omit<DiffEditorProps, 'beforeMount'>,
) {
  return (
    <DiffEditor
      {...props}
      beforeMount={() => {
        void loadMonaco();
        beforeEditorMount();
      }}
    />
  );
}
