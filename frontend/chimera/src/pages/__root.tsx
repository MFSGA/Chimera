import { AppContainer } from '@/components/app/app-container';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { check } from '@tauri-apps/plugin-updater';
import { lazy } from 'react';

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
    <div>
      <h1>Hello World</h1>
      <div onClick={getVersion}>get version</div>

      <AppContainer>
        <Outlet />
        <TanStackRouterDevtools />
      </AppContainer>
    </div>
  );
}
