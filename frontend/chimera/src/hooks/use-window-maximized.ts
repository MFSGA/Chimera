import { getSystem } from '@chimera/ui';
import { useSuspenseQuery } from '@tanstack/react-query';
import { TauriEvent } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useCallback, useEffect } from 'react';

const appWindow = getCurrentWebviewWindow();
const isMacOS = getSystem() === 'macos';
const IS_MAXIMIZED_QUERY_KEY = 'isMaximized';

export default function useWindowMaximized() {
  const query = useSuspenseQuery({
    queryKey: [IS_MAXIMIZED_QUERY_KEY],
    queryFn: () =>
      isMacOS ? appWindow.isFullscreen() : appWindow.isMaximized(),
  });

  const toggleMaximize = useCallback(async () => {
    await appWindow.toggleMaximize();
    await query.refetch();
  }, [query]);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    appWindow
      .listen(TauriEvent.WINDOW_RESIZED, () => query.refetch())
      .then((dispose) => {
        if (disposed) {
          dispose();
        } else {
          unlisten = dispose;
        }
      })
      .catch(console.error);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [query]);

  return {
    isMaximized: query.data,
    toggleMaximize,
    ...query,
  };
}
