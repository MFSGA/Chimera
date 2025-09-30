import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/rules')({
  component: RulesPage,
});

function RulesPage() {
  const { t } = useTranslation();

  return <div>{t('page_rules')}</div>;
}
