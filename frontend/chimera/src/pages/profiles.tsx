import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  const { t } = useTranslation();

  return <div>{t('page_profiles')}</div>;
}
