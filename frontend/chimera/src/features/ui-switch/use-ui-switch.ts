import {
  commands,
  unwrapResult,
  useSetting,
  type WindowType,
} from '@chimera/interface';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

export const useUiSwitch = () => {
  const windowType = useSetting('window_type');

  const switchTo = useLockFn(async (target: WindowType) => {
    try {
      await windowType.upsert(target);

      if (target === 'main') {
        unwrapResult(await commands.createMainWindow());
      } else {
        unwrapResult(await commands.createLegacyWindow());
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
