import { useSettings } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CssBaseline } from '@mui/material';
import { StyledEngineProvider } from '@mui/material/styles';
import { createFileRoute, useLocation } from '@tanstack/react-router';
import { useAtom, useSetAtom } from 'jotai';
import { PropsWithChildren, useEffect } from 'react';
import { SWRConfig } from 'swr';
import { AppContainer } from '@/components/app/app-container';
import LocalesProvider from '@/components/app/locales-provider';
import NoticeProvider from '@/components/layout/notice-provider';
import PageTransition from '@/components/layout/page-transition';
import SchemeProvider from '@/components/layout/scheme-provider';
import { ThemeModeProvider } from '@/components/layout/use-custom-theme';
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper';
import { UpdaterProvider } from '@/hooks/use-updater';
import { FileRouteTypes } from '@/routeTree.gen';
import { atomIsDrawer, memorizedRoutePathAtom } from '@/store';

export const Route = createFileRoute('/(legacy)')({
  component: Layout,
});

const QueryLoaderProvider = ({ children }: PropsWithChildren) => {
  const {
    query: { isLoading },
  } = useSettings();

  return isLoading ? null : children;
};

function Layout() {
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

  return (
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
            <NoticeProvider />
            <SchemeProvider />
            <UpdaterDialog />
            <UpdaterProvider />

            <AppContainer isDrawer={isDrawer}>
              <PageTransition
                className={cn('absolute inset-4 top-10', !isDrawer && 'left-0')}
              />
            </AppContainer>
          </ThemeModeProvider>
        </StyledEngineProvider>
      </QueryLoaderProvider>
    </SWRConfig>
  );
}
