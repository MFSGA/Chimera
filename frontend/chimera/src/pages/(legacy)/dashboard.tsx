import { useClashWSContext } from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import Grid from '@mui/material/Grid';
import { createFileRoute } from '@tanstack/react-router';
import { useEffect } from 'react';
import DataPanel from '@/components/dashboard/data-panel';
import HealthPanel from '@/components/dashboard/health-panel';
import ProxyShortcuts from '@/components/dashboard/proxy-shortcuts';
import ServiceShortcuts from '@/components/dashboard/service-shortcuts';
import { useVisibility } from '@/hooks/use-visibility';
import * as m from '@/paraglide/messages';

export const Route = createFileRoute('/(legacy)/dashboard')({
  component: Dashboard,
});

function Dashboard() {
  const visible = useVisibility();
  const { setRecordTraffic } = useClashWSContext();

  useEffect(() => {
    setRecordTraffic(visible);
  }, [setRecordTraffic, visible]);

  return (
    <BasePage title={m.navbar_label_dashboard()}>
      <Grid container spacing={2}>
        <DataPanel visible={visible} />
        <HealthPanel />
        <ProxyShortcuts />
        <ServiceShortcuts />
      </Grid>
    </BasePage>
  );
}
