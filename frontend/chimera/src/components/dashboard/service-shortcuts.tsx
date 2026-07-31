import { useCoreStatus, useSystemService } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import { Box, CircularProgress, Paper, Tooltip } from '@mui/material';
import Grid from '@mui/material/Grid';
import type { SxProps, Theme } from '@mui/material/styles';
import dayjs from 'dayjs';
import { useAtomValue } from 'jotai';
import { isObject } from 'lodash-es';
import { useMemo } from 'react';
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
    const status = coreStatusQuery.data?.status ?? { Stopped: null };
    if (
      isObject(status) &&
      Object.prototype.hasOwnProperty.call(status, 'Stopped')
    ) {
      const { Stopped } = status;
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
        coreStatusQuery.data?.type === 'normal'
          ? m.dashboard_widget_core_status_running_by_child_process()
          : m.dashboard_widget_core_status_running_by_service(),
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
