import { StyledEngineProvider } from '@mui/material/styles';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { lazy } from 'react';
import { SWRConfig } from 'swr';
import { AppContainer } from '@/components/app/app-container';

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
  return (
    <SWRConfig
      value={{
        errorRetryCount: 5,
        revalidateOnMount: true,
        revalidateOnFocus: true,
        refreshInterval: 5000,
      }}
    >
      <StyledEngineProvider injectFirst>
        <AppContainer>
          <Outlet />
          <TanStackRouterDevtools />
        </AppContainer>
      </StyledEngineProvider>
    </SWRConfig>
  );
}
