import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/providers')({
  component: ProvidersPage,
});

function ProvidersPage() {
  const { t } = useTranslation();

  return <div>{t('page_providers')}</div>;
}
