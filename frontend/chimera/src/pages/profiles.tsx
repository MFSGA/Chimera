import { SidePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
import { QuickImport } from '@/components/profiles/quick-import';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  const { t } = useTranslation();
  // todo: optimize the components
  return (
    <SidePage>
      <div className="flex flex-col gap-4 p-6">
        <QuickImport />
      </div>
    </SidePage>
  );
}
