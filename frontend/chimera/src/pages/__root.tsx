import { RootProvider } from '@chimera/interface';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { emit } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useMount } from 'ahooks';
import { lazy } from 'react';
import { useNyanpasuStorageSubscribers } from '@/hooks/use-store';
import 'dayjs/locale/ru';
import 'dayjs/locale/zh-cn';
import 'dayjs/locale/zh-tw';
import dayjs from 'dayjs';
import customParseFormat from 'dayjs/plugin/customParseFormat';
import relativeTime from 'dayjs/plugin/relativeTime';

dayjs.extend(relativeTime);
dayjs.extend(customParseFormat);

const TanStackRouterDevtools = import.meta.env.PROD
  ? () => null // Render nothing in production
  : lazy(() =>
      // Lazy load in development
      import('@tanstack/react-router-devtools').then((res) => ({
        default: res.TanStackRouterDevtools,
        // For Embedded Mode
        // default: res.TanStackRouterDevtoolsPanel
      })),
    );

export const Route = createRootRoute({
  component: App,
  // errorComponent: Catch,
  // pendingComponent: Pending,
});

export default function App() {
  useNyanpasuStorageSubscribers();

  useMount(() => {
    const appWindow = getCurrentWebviewWindow();
    Promise.all([
      appWindow.show(),
      appWindow.unminimize(),
      appWindow.setFocus(),
    ])
      .catch((error) => {
        console.error(error);
      })
      .finally(() => {
        emit('react_app_mounted').catch((error) => {
          console.error(error);
        });
      });
  });

  return (
    <RootProvider>
      <Outlet />
      <TanStackRouterDevtools />
    </RootProvider>
  );
}
