import { BasePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { lazy, Suspense, useRef, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import LogHeader from '@/components/logs/log-header';
import { LogProvider } from '@/components/logs/log-provider';

export const Route = createFileRoute('/logs')({
  component: LogRoutePage,
});

function LogRoutePage() {
  const { t } = useTranslation();
  const viewportRef = useRef<HTMLDivElement>(null);
  const Component = lazy(() => import('@/components/logs/log-page'));

  return (
    <LogProvider>
      <BasePage
        full
        title={t('Logs')}
        header={<LogHeader />}
        viewportRef={viewportRef}
      >
        <Suspense fallback={null}>
          <Component scrollRef={viewportRef as RefObject<HTMLElement>} />
        </Suspense>
      </BasePage>
    </LogProvider>
  );
}
