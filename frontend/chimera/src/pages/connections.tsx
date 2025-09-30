import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/connections')({
  component: Connections,
});

function Connections() {
  const { t } = useTranslation();

  return <div>{t('page_connections')}</div>;
}
