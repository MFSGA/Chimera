import { useClashWSContext } from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import Grid from '@mui/material/Grid';
import { createFileRoute } from '@tanstack/react-router';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import DataPanel from '@/components/dashboard/data-panel';
import ProxyShortcuts from '@/components/dashboard/proxy-shortcuts';
import ServiceShortcuts from '@/components/dashboard/service-shortcuts';
import { useVisibility } from '@/hooks/use-visibility';

export const Route = createFileRoute('/(legacy)/dashboard')({
  component: Dashboard,
});

function Dashboard() {
  const { t } = useTranslation();
  const visible = useVisibility();
  const { setRecordTraffic } = useClashWSContext();

  useEffect(() => {
    setRecordTraffic(visible);
  }, [setRecordTraffic, visible]);

  return (
    <BasePage title={t('Dashboard')}>
      <Grid container spacing={2}>
        <DataPanel visible={visible} />
        <ProxyShortcuts />
        <ServiceShortcuts />
      </Grid>
    </BasePage>
  );
}
