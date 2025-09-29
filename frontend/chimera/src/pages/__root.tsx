import { StyledEngineProvider } from '@mui/material/styles';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { check } from '@tauri-apps/plugin-updater';
import { lazy } from 'react';
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

async function getVersion() {
  const update = await check();
  console.log(update);
  if (update) {
    console.log('Update available:');

    // await update.downloadAndInstall();
    // await relaunch();
  }
}

export default function App() {
  return (
    /* the following is just for test tailwin */
    <div className="min-h-screen bg-slate-50 font-sans text-slate-900">
      <div
        className="cursor-pointer px-3 py-2 text-sm font-semibold text-blue-600 transition hover:text-blue-700"
        onClick={getVersion}
      >
        get version
      </div>
      <StyledEngineProvider injectFirst>
        <AppContainer>
          <Outlet />
          <TanStackRouterDevtools />
        </AppContainer>
      </StyledEngineProvider>
    </div>
  );
}
