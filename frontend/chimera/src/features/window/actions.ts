import { commands, unwrapResult } from '@chimera/interface';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { platform } from '@tauri-apps/plugin-os';

const currentWindow = getCurrentWebviewWindow();

export const saveCurrentWindowState = async () => {
  if (platform() !== 'windows') {
    return true;
  }

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
