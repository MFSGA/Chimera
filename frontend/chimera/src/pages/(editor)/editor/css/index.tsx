import '@/services/monaco';
import MonacoEditor from '@monaco-editor/react';
import { createFileRoute } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useEffect, useState } from 'react';
import { useExperimentalThemeContext } from '@/components/providers/theme-provider';
import useCustomCss from '@/hooks/use-custom-css';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { registerCssDataSlotCompletion } from '@/utils/monaco-css';
import { message } from '@/utils/notification';
import ActionButton from '../_modules/action-button';
import Header from '../_modules/header';
import { MONACO_FONT_FAMILY } from '../_modules/utils';

const currentWindow = getCurrentWebviewWindow();

const EMPTY_CSS_TEMPLATE = `/* Welcome to Clash Chimera CSS/LESS Editor! */
/* You can write standard CSS or LESS syntax to customize the appearance of the app. */
/* For example, you can target stable data-slot selectors: */

/*
@primary: #ff69b4;

[data-slot="window-control"] {
  background-color: @primary;
}
*/

/* Feel free to experiment and make the app your own. */
`;

export const Route = createFileRoute('/(editor)/editor/css/')({
  component: RouteComponent,
});

function RouteComponent() {
  const { themeMode } = useExperimentalThemeContext();
  const customCss = useCustomCss();
  const [editorValue, setEditorValue] = useState('');
  const [initialized, setInitialized] = useState(false);

  useEffect(() => {
    if (!initialized && !customCss.isLoading) {
      setEditorValue(customCss.value || EMPTY_CSS_TEMPLATE);
      setInitialized(true);
    }
  }, [customCss.value, customCss.isLoading, initialized]);

  const handleSave = useLockFn(async (close?: boolean) => {
    try {
      await customCss.upsert(editorValue);
      if (close) {
        await currentWindow.close();
      }
    } catch {
      await message(m.custom_css_save_error(), { kind: 'error' });
    }
  });

  const handleClear = useLockFn(async () => {
    await customCss.upsert('');
    setEditorValue('');
  });

  const handleCancel = useLockFn(async () => {
    await currentWindow.close();
  });

  return (
    <>
      <Header title="Clash Chimera - CSS Editor" />

      <div className="min-h-0 flex-1" data-slot="editor-content">
        <MonacoEditor
          className="h-full w-full"
          language="less"
          value={editorValue}
          theme={themeMode === 'light' ? 'vs' : 'vs-dark'}
          beforeMount={registerCssDataSlotCompletion}
          onChange={(value) => setEditorValue(value ?? '')}
          options={{
            minimap: { enabled: false },
            wordWrap: 'on',
            automaticLayout: true,
            fontFamily: MONACO_FONT_FAMILY,
            quickSuggestions: { strings: true, comments: false, other: true },
          }}
        />
      </div>

      <div
        className="bg-background flex h-12 shrink-0 items-center gap-2 px-3"
        data-slot="editor-footer-actions"
      >
        <ActionButton onClick={() => void handleClear()}>
          {m.common_clear()}
        </ActionButton>

        <div className="flex-1" />

        <ActionButton onClick={() => void handleCancel()}>
          {m.common_cancel()}
        </ActionButton>

        <ActionButton
          className="px-5"
          variant="flat"
          onClick={() => void handleSave(false)}
        >
          {m.common_apply()}
        </ActionButton>

        <ActionButton
          className="px-5"
          variant="flat"
          onClick={() => void handleSave(true)}
        >
          {m.common_save()}
        </ActionButton>
      </div>
    </>
  );
}
