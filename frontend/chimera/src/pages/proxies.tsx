import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
});

function ProxyPage() {
  const { t } = useTranslation();

  return <div>{t('page_proxies')}</div>;
}
