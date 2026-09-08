import { commands, unwrapResult } from '@chimera/interface';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';

const currentWindow = getCurrentWebviewWindow();

export const saveCurrentWindowState = async () => {
  try {
    unwrapResult(await commands.saveWindowSizeState(currentWindow.label));
  } catch (error) {
    console.error('Failed to save window state before close', error);
  }

  return true;
};

export const closeCurrentWindow = async () => {
  await saveCurrentWindowState();
  await currentWindow.close();
};
