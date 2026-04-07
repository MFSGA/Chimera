import { useClashWSContext } from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import Grid from '@mui/material/Grid';
import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';
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

  setRecordTraffic(visible);

  return (
    <BasePage title={t('Dashboard')}>
      <Grid container spacing={2}>
        <ProxyShortcuts />
        <ServiceShortcuts />
      </Grid>
    </BasePage>
  );
}
