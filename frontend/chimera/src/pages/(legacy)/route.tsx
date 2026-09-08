import { useSettings } from '@chimera/interface';
import { cn, useBreakpoint } from '@chimera/ui';
import { CssBaseline } from '@mui/material';
import { StyledEngineProvider } from '@mui/material/styles';
import { createFileRoute, useLocation } from '@tanstack/react-router';
import { useAtom, useSetAtom } from 'jotai';
import { PropsWithChildren, useEffect } from 'react';
import { AppContainer } from '@/components/app/app-container';
import NoticeProvider from '@/components/layout/notice-provider';
import PageTransition from '@/components/layout/page-transition';
import { ThemeModeProvider } from '@/components/layout/use-custom-theme';
import ChimeraUpdateProvider from '@/components/providers/chimera-update-provider';
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper';
import { LegacyUpdaterAdapter } from '@/features/updater/legacy-updater-adapter';
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
  const breakpoint = useBreakpoint();
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
    setIsDrawer(breakpoint === 'sm' || breakpoint === 'xs');
  }, [breakpoint, setIsDrawer]);

  return (
    <QueryLoaderProvider>
      <ChimeraUpdateProvider>
        <StyledEngineProvider injectFirst>
          <ThemeModeProvider>
            <CssBaseline />
            <NoticeProvider />
            <UpdaterDialog />
            <LegacyUpdaterAdapter />

            <AppContainer isDrawer={isDrawer}>
              <PageTransition
                className={cn('absolute inset-4 top-10', !isDrawer && 'left-0')}
              />
            </AppContainer>
          </ThemeModeProvider>
        </StyledEngineProvider>
      </ChimeraUpdateProvider>
    </QueryLoaderProvider>
  );
}
