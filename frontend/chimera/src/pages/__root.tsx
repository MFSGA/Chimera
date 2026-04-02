import { RootProvider, useSettings } from '@chimera/interface';
import { CssBaseline } from '@mui/material';
import { StyledEngineProvider } from '@mui/material/styles';
import { createRootRoute, useLocation } from '@tanstack/react-router';
import { emit } from '@tauri-apps/api/event';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useMount } from 'ahooks';
import { lazy, PropsWithChildren, useEffect } from 'react';
import { SWRConfig } from 'swr';
import { AppContainer } from '@/components/app/app-container';
import LocalesProvider from '@/components/app/locales-provider';
import MutationProvider from '@/components/layout/mutation-provider';
import NoticeProvider from '@/components/layout/notice-provider';
import SchemeProvider from '@/components/layout/scheme-provider';
import { ThemeModeProvider } from '@/components/layout/use-custom-theme';
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper';
import { useNyanpasuStorageSubscribers } from '@/hooks/use-store';
import { UpdaterProvider } from '@/hooks/use-updater';
import { atomIsDrawer, memorizedRoutePathAtom } from '@/store';
import 'dayjs/locale/ru';
import 'dayjs/locale/zh-cn';
import 'dayjs/locale/zh-tw';
import { cn } from '@chimera/ui';
import dayjs from 'dayjs';
import customParseFormat from 'dayjs/plugin/customParseFormat';
import relativeTime from 'dayjs/plugin/relativeTime';
import { useAtom, useSetAtom } from 'jotai';
import PageTransition from '@/components/layout/page-transition';
import { FileRouteTypes } from '@/routeTree.gen';

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

const QueryLoaderProvider = ({ children }: PropsWithChildren) => {
  const {
    query: { isLoading },
  } = useSettings();

  return isLoading ? null : children;
};

export default function App() {
  useNyanpasuStorageSubscribers();

  const [isDrawer, setIsDrawer] = useAtom(atomIsDrawer);
  const setMemorizedPath = useSetAtom(memorizedRoutePathAtom);
  const pathname = useLocation({
    select: (location) => location.pathname,
  });

  useEffect(() => {
    if (pathname !== '/') {
      setMemorizedPath(pathname as FileRouteTypes['to']);
    }
  }, [pathname, setMemorizedPath]);

  useEffect(() => {
    const syncDrawerState = () => {
      setIsDrawer(window.innerWidth < 900);
    };

    syncDrawerState();
    window.addEventListener('resize', syncDrawerState);

    return () => {
      window.removeEventListener('resize', syncDrawerState);
    };
  }, [setIsDrawer]);

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
      <SWRConfig
        value={{
          errorRetryCount: 5,
          revalidateOnMount: true,
          revalidateOnFocus: true,
          refreshInterval: 5000,
        }}
      >
        <QueryLoaderProvider>
          <StyledEngineProvider injectFirst>
            <ThemeModeProvider>
              <CssBaseline />
              <LocalesProvider />
              <MutationProvider>
                <NoticeProvider />
                <SchemeProvider />
                <UpdaterDialog />
                <UpdaterProvider />

                <AppContainer isDrawer={isDrawer}>
                  <PageTransition
                    className={cn(
                      'absolute inset-4 top-10',
                      !isDrawer && 'left-0',
                    )}
                  />

                  <TanStackRouterDevtools />
                </AppContainer>
              </MutationProvider>
            </ThemeModeProvider>
          </StyledEngineProvider>
        </QueryLoaderProvider>
      </SWRConfig>
    </RootProvider>
  );
}
