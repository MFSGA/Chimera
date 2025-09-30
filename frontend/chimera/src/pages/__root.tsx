import { StyledEngineProvider } from '@mui/material/styles';
import { createRootRoute, Outlet } from '@tanstack/react-router';
import { useAtomValue } from 'jotai';
import { lazy, useEffect } from 'react';
import { SWRConfig } from 'swr';
import { AppContainer } from '@/components/app/app-container';
import i18n from '@/services/i18n';
import { languageAtom } from '@/store';

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

const LanguageSync = () => {
  const language = useAtomValue(languageAtom);

  useEffect(() => {
    i18n.changeLanguage(language);
    if (typeof document !== 'undefined') {
      document.documentElement.lang = language;
    }
  }, [language]);

  return null;
};

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
        <LanguageSync />
        <AppContainer>
          <Outlet />
          <TanStackRouterDevtools />
        </AppContainer>
      </StyledEngineProvider>
    </SWRConfig>
  );
}
