import { getCoreStatus, useSystemService } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import { Box, CircularProgress, Paper, Tooltip } from '@mui/material';
import Grid from '@mui/material/Grid';
import type { SxProps, Theme } from '@mui/material/styles';
import dayjs from 'dayjs';
import { useAtomValue } from 'jotai';
import { isObject } from 'lodash-es';
import { useMemo } from 'react';
import useSWR from 'swr';
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

  // TODO: refactor to use tanstack query
  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  });

  const status: Status = useMemo(() => {
    switch (serviceStatus?.status) {
      case 'running': {
        return {
          label: m.dashboard_widget_core_service_running(),
          sx: ((theme) => ({
            backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
            ...theme.applyStyles('dark', {
              backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
            }),
          })) as SxProps<Theme>,
        };
      }

      case 'stopped': {
        return {
          label: m.dashboard_widget_core_service_stopped(),
          sx: ((theme) => ({
            backgroundColor: alpha(theme.vars.palette.error.light, 0.3),
            ...theme.applyStyles('dark', {
              backgroundColor: alpha(theme.vars.palette.error.dark, 0.3),
            }),
          })) as SxProps<Theme>,
        };
      }

      case 'not_installed':
      default: {
        return {
          label: m.dashboard_widget_core_service_not_installed(),
          sx: ((theme) => ({
            backgroundColor: theme.vars.palette.grey[100],
            ...theme.applyStyles('dark', {
              backgroundColor: theme.vars.palette.background.paper,
            }),
          })) as SxProps<Theme>,
        };
      }
    }
  }, [serviceStatus]);

  const coreStatus: Status = useMemo(() => {
    const status = coreStatusSWR.data || [{ Stopped: null }, 0, 'normal'];
    if (
      isObject(status[0]) &&
      Object.prototype.hasOwnProperty.call(status[0], 'Stopped')
    ) {
      const { Stopped } = status[0];
      return {
        label:
          !!Stopped && Stopped.trim()
            ? m.dashboard_widget_core_stopped_with_message({ message: Stopped })
            : m.dashboard_widget_core_status_stopped(),
        sx: ((theme) => ({
          backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
          ...theme.applyStyles('dark', {
            backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
          }),
        })) as SxProps<Theme>,
      };
    }
    return {
      label:
        status[2] === 'normal'
          ? m.dashboard_widget_core_status_running_by_child_process()
          : m.dashboard_widget_core_status_running_by_service(),
      sx: ((theme) => ({
        backgroundColor: alpha(theme.vars.palette.success.light, 0.3),
        ...theme.applyStyles('dark', {
          backgroundColor: alpha(theme.vars.palette.success.dark, 0.3),
        }),
      })) as SxProps<Theme>,
    };
  }, [coreStatusSWR.data]);

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
            <div className="text-center font-bold">Service</div>

            <div className="flex w-full flex-col gap-2">
              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={status.sx}
              >
                <div>Service Status</div>
                <div>{status.label}</div>
              </Box>

              <Box
                className="flex w-full justify-center gap-[2px] rounded-2xl py-2"
                sx={coreStatus.sx}
              >
                <div>{m.dashboard_widget_core_status()}</div>
                <Tooltip
                  title={
                    !!coreStatusSWR.data?.[1] &&
                    `Last changed ${dayjs(coreStatusSWR.data[1]).fromNow()}`
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

            <div>Loading...</div>
          </div>
        )}
      </Paper>
    </Grid>
  );
};

export default ServiceShortcuts;
