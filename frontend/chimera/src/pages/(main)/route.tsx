import { cn } from '@chimera/utils';
import { CssBaseline } from '@mui/material';
import { StyledEngineProvider } from '@mui/material/styles';
import { createFileRoute } from '@tanstack/react-router';
import packageJson from '@root/package.json';
import ChimeraUpdateProvider from '@/components/providers/chimera-update-provider';
import ContextMenuProvider from '@/components/providers/context-menu-provider';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import useIsMobile from '@/hooks/use-is-moblie';
import Header from './_modules/-header';
import {
  DefaultNavbar,
  LegacyNavbarButton,
  MobileNavbar,
} from './_modules/-navbar';

export const Route = createFileRoute('/(main)')({
  component: RouteComponent,
});

const AppContent = () => {
  return (
    <AnimatedOutletPreset
      className={cn(
        'flex min-h-0 flex-1 flex-col overflow-hidden',
        '[&>div]:min-h-0 [&>div]:flex-1',
      )}
      data-slot="app-content"
    />
  );
};

function RouteComponent() {
  const isMobile = useIsMobile();

  return (
    <ChimeraUpdateProvider>
      <ContextMenuProvider>
        <StyledEngineProvider injectFirst>
          <CssBaseline />

          <div
            className={cn(
              'flex max-h-dvh min-h-dvh flex-col overflow-hidden',
              'bg-mixed-background',
            )}
            data-slot="app-root"
            data-app-version={packageJson.version}
          >
            <Header className="shrink-0" />

            <div
              className="flex min-h-0 flex-1 flex-col"
              data-slot="app-content-container"
            >
              {!isMobile && (
                <div
                  className={cn(
                    'flex h-12 shrink-0 items-center gap-2 px-3',
                    'bg-primary-container dark:bg-on-primary',
                  )}
                  data-slot="app-navbar"
                >
                  <DefaultNavbar />
                  <LegacyNavbarButton />
                </div>
              )}

              <AppContent />

              {isMobile && (
                <div
                  className={cn(
                    'flex h-16 shrink-0 items-center justify-between gap-2 px-3',
                    'bg-primary-container dark:bg-scrim',
                  )}
                  data-slot="app-navbar-mobile"
                >
                  <MobileNavbar />
                </div>
              )}
            </div>
          </div>
        </StyledEngineProvider>
      </ContextMenuProvider>
    </ChimeraUpdateProvider>
  );
}
