import { useClashConfig, useSetting, useSystemProxy } from '@chimera/interface';
import { NetworkPing, SettingsEthernet } from '@mui/icons-material';
import { Chip, Paper, type ChipProps } from '@mui/material';
import Grid from '@mui/material/Grid';
import { useAtomValue } from 'jotai';
import { useMemo } from 'react';
import { PaperSwitchButton } from '@/components/setting/modules/system-proxy';
import { getProxyStatus } from '@/features/dashboard/proxy-status';
import {
  useSystemProxyAction,
  useTunModeAction,
} from '@/features/system-proxy/use-proxy-settings';
import * as m from '@/paraglide/messages';
import { atomIsDrawer } from '@/store';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const TitleComp = () => {
  const {
    query: { data: clashConfigs },
  } = useClashConfig();
  const systemProxy = useSystemProxy();
  const { value: enableSystemProxy } = useSetting('enable_system_proxy');
  const { value: enableTunMode } = useSetting('enable_tun_mode');
  const mixedPort = clashConfigs?.['mixed-port'];

  const status = useMemo<{
    label: string;
    color: ChipProps['color'];
  }>(() => {
    const proxyStatus = getProxyStatus({
      enableSystemProxy,
      enableTunMode,
      systemProxyEnabled: systemProxy.data?.enable,
      systemProxyServer: systemProxy.data?.server,
      mixedPort,
    });

    switch (proxyStatus) {
      case 'system':
        return {
          label: m.dashboard_widget_proxy_status_success_system(),
          color: 'success',
        };
      case 'tun':
        return {
          label: m.dashboard_widget_proxy_status_success_tun(),
          color: 'success',
        };
      case 'occupied':
        return {
          label: m.dashboard_widget_proxy_status_occupied(),
          color: 'warning',
        };
      default:
        return {
          label: m.dashboard_widget_proxy_status_disabled(),
          color: 'error',
        };
    }
  }, [enableSystemProxy, enableTunMode, mixedPort, systemProxy.data]);

  return (
    <div className="flex items-center gap-2 px-1">
      <div>{m.dashboard_widget_proxy_status()}</div>

      <Chip
        label={status.label}
        color={status.color}
        className="!h-5"
        sx={{
          span: {
            padding: '0 8px',
          },
        }}
      />
    </div>
  );
};

export const ProxyShortcuts = () => {
  const isDrawer = useAtomValue(atomIsDrawer);

  const systemProxy = useSystemProxyAction();
  const tunMode = useTunModeAction();

  const handleSystemProxy = async () => {
    try {
      await systemProxy.execute();
    } catch (error) {
      await message(
        `Activation System Proxy failed! \n Error: ${formatError(error)}`,
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  };

  const handleTunMode = async () => {
    try {
      await tunMode.execute();
    } catch (error) {
      await message(
        `Activation TUN Mode failed! \n Error: ${formatError(error)}`,
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  };

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
        <TitleComp />

        <div className="flex gap-3">
          <div className="!w-full">
            <PaperSwitchButton
              checked={systemProxy.isActive}
              onClick={handleSystemProxy}
              disabled={systemProxy.isPending}
            >
              <div className="flex flex-col gap-2">
                <NetworkPing />

                <div>{m.settings_system_proxy_system_proxy_label()}</div>
              </div>
            </PaperSwitchButton>
          </div>

          <div className="!w-full">
            <PaperSwitchButton
              checked={tunMode.isActive}
              onClick={handleTunMode}
              disabled={tunMode.isPending}
            >
              <div className="flex flex-col gap-2">
                <SettingsEthernet />

                <div>{m.settings_system_proxy_tun_mode_label()}</div>
              </div>
            </PaperSwitchButton>
          </div>
        </div>
      </Paper>
    </Grid>
  );
};

export default ProxyShortcuts;
