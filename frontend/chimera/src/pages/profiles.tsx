import { SidePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  const { t } = useTranslation();
  // todo: optimize the components
  return <SidePage></SidePage>;
}
