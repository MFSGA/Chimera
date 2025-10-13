import { useProfile } from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { Box } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { QuickImport } from '@/components/profiles/quick-import';
import { filterProfiles } from '@/components/profiles/utils';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  const { t } = useTranslation();

  const { query } = useProfile();

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);
  // todo: optimize the components

  return (
    <SidePage>
      <div className="flex flex-col gap-4 p-6">
        <QuickImport />

        {profiles && <Box>profiles get</Box>}
      </div>
    </SidePage>
  );
}
