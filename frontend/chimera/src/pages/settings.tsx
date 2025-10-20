import { BasePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { lazy } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/settings')({
  component: SettingPage,
});

function SettingPage() {
  const { t } = useTranslation();
  // vital
  const Component = lazy(() => import('@/components/setting/setting-page'));

  return (
    <BasePage title={t('Settings')}>
      <Component />
    </BasePage>
  );
}
