import { RootProvider, useSettings } from '@chimera/interface';
import { CssBaseline } from '@mui/material';
import { StyledEngineProvider } from '@mui/material/styles';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { lazy, PropsWithChildren } from 'react';
import { SWRConfig } from 'swr';
import { AppContainer } from '@/components/app/app-container';
import LocalesProvider from '@/components/app/locales-provider';
import { ThemeModeProvider } from '@/components/layout/use-custom-theme';
import UpdaterDialog from '@/components/updater/updater-dialog-wrapper';
import { UpdaterProvider } from '@/hooks/use-updater';
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

const QueryLoaderProvider = ({ children }: PropsWithChildren) => {
  const {
    query: { isLoading },
  } = useSettings();

  return isLoading ? null : children;
};

export default function App() {
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
              {/* 4 */}
              <CssBaseline />
              {/* 3*/}
              <LocalesProvider />

              <UpdaterDialog />
              <UpdaterProvider />

              <AppContainer>
                <Outlet />
                <TanStackRouterDevtools />
              </AppContainer>
            </ThemeModeProvider>
          </StyledEngineProvider>
        </QueryLoaderProvider>
      </SWRConfig>
    </RootProvider>
  );
}
