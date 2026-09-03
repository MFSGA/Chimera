import { useCoreStatus, useSystemService } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import { Box, CircularProgress, Paper, Tooltip } from '@mui/material';
import Grid from '@mui/material/Grid';
import type { SxProps, Theme } from '@mui/material/styles';
import dayjs from 'dayjs';
import { useAtomValue } from 'jotai';
import { useMemo } from 'react';
import {
  getRunningCoreMessage,
  getServiceStatusMessage,
  getStoppedCoreReason,
} from '@/features/dashboard/core-service-status';
import * as m from '@/paraglide/messages';
import { atomIsDrawer } from '@/store';

type Status = {
  label: string;
  sx: SxProps<Theme>;
};

export const ServiceShortcuts = () => {
  const isDrawer = useAtomValue(atomIsDrawer);

  const {
    query: { data: serviceStatus },
  } = useSystemService();

  const coreStatusQuery = useCoreStatus();

  const status: Status = useMemo(() => {
    const label = getServiceStatusMessage(serviceStatus?.status);

    if (serviceStatus?.status === 'running') {
      return {
        label,
        sx: ((theme) => ({
          backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
          ...theme.applyStyles('dark', {
            backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
          }),
        })) as SxProps<Theme>,
      };
    }

    if (serviceStatus?.status === 'stopped') {
      return {
        label,
        sx: ((theme) => ({
          backgroundColor: alpha(theme.vars.palette.error.light, 0.3),
          ...theme.applyStyles('dark', {
            backgroundColor: alpha(theme.vars.palette.error.dark, 0.3),
          }),
        })) as SxProps<Theme>,
      };
    }

    return {
      label,
      sx: ((theme) => ({
        backgroundColor: theme.vars.palette.grey[100],
        ...theme.applyStyles('dark', {
          backgroundColor: theme.vars.palette.background.paper,
        }),
      })) as SxProps<Theme>,
    };
  }, [serviceStatus?.status]);

  const coreStatus: Status = useMemo(() => {
    const stoppedReason = getStoppedCoreReason(coreStatusQuery.data?.status);
    const isStopped = coreStatusQuery.data?.status !== 'Running';

    return {
      label: isStopped
        ? stoppedReason?.trim()
          ? m.dashboard_widget_core_stopped_with_message({
              message: stoppedReason,
            })
          : m.dashboard_widget_core_status_stopped()
        : getRunningCoreMessage({ coreType: coreStatusQuery.data?.type }),
      sx: ((theme) => ({
        backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
        ...theme.applyStyles('dark', {
          backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
        }),
      })) as SxProps<Theme>,
    };
  }, [coreStatusQuery.data]);

  return (
    <Grid
      size={{
        sm: isDrawer ? 6 : 12,
        md: 6,
        lg: 4,
        xl: 3,
      }}
    >
      <Paper className="flex !h-full flex-col justify-between gap-2 !rounded-3xl p-3">
        {serviceStatus ? (
          <>
            <div className="text-center font-bold">
              {m.settings_system_proxy_system_service_ctrl_label()}
            </div>

            <div className="flex w-full flex-col gap-2">
              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={status.sx}
              >
                <div>{m.settings_system_service_status_label()}</div>
                <div>{status.label}</div>
              </Box>

              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={coreStatus.sx}
              >
                <div>{m.dashboard_widget_core_status()}</div>
                <Tooltip
                  title={
                    !!coreStatusQuery.data?.startAt &&
                    `Last changed ${dayjs(coreStatusQuery.data.startAt).fromNow()}`
                  }
                >
                  <div>{coreStatus.label}</div>
                </Tooltip>
              </Box>
            </div>
          </>
        ) : (
          <div className="flex w-full flex-col items-center justify-center gap-2">
            <CircularProgress />

            <div>{m.common_loading()}</div>
          </div>
        )}
      </Paper>
    </Grid>
  );
};

export default ServiceShortcuts;
