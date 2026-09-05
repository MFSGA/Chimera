import {
  commands,
  unwrapResult,
  useSetting,
  type WindowType,
} from '@chimera/interface';
import {
  getCurrentWebviewWindow,
  WebviewWindow,
} from '@tauri-apps/api/webviewWindow';
import { saveCurrentWindowState } from '@/features/window/actions';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();
const WINDOW_CREATE_TIMEOUT_MS = 5000;

const waitForWindow = async (label: string) => {
  const deadline = Date.now() + WINDOW_CREATE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const window = await WebviewWindow.getByLabel(label);
    if (window) return window;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }

  throw new Error(`Window ${label} was not created in time`);
};

export const useUiSwitch = () => {
  const windowType = useSetting('window_type');

  const switchTo = useLockFn(async (target: WindowType) => {
    try {
      const targetLabel = target === 'main' ? 'main' : 'legacy';
      const existingTarget = await WebviewWindow.getByLabel(targetLabel);

      await saveCurrentWindowState();
      if (target === 'main') {
        unwrapResult(await commands.createMainWindow());
      } else {
        unwrapResult(await commands.createLegacyWindow());
      }

      const targetWindow = await waitForWindow(targetLabel);
      try {
        await windowType.upsert(target);
      } catch (error) {
        if (!existingTarget) {
          await targetWindow.close().catch(console.error);
        }
        throw error;
      }

      await currentWindow.close();
      return true;
    } catch (error) {
      await message(
        `Failed to open ${target === 'main' ? 'main' : 'legacy'} UI: ${formatError(error)}`,
        {
          kind: 'error',
          title: m.common_error(),
        },
      );
      return false;
    }
  });

  return {
    isPending: windowType.isPending,
    switchToMain: () => switchTo('main'),
    switchToLegacy: () => switchTo('legacy'),
  };
};
