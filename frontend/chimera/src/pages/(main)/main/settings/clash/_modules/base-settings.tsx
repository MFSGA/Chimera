import {
  openUWPTool,
  useClashConfig,
  useRuntimeProfile,
  useSetting,
  type TunStack,
} from '@chimera/interface';
import { useMemo } from 'react';
import {
  SelectorCard,
  SwitchCard,
} from '@/components/settings/setting-control';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '@/components/settings/settings-card';
import { Button } from '@/components/ui/button';
import { useCoreType } from '@/hooks/use-store';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import getSystem from '@/utils/get-system';
import { message } from '@/utils/notification';

export const AllowLanSwitch = () => {
  const { query, upsert } = useClashConfig();

  return (
    <SwitchCard
      label={m.settings_clash_settings_allow_lan_label()}
      checked={Boolean(query.data?.['allow-lan'])}
      loading={upsert.isPending}
      onCheckedChange={(checked) =>
        void upsert.mutateAsync({ 'allow-lan': checked }).catch((error) =>
          message(formatError(error), {
            title: m.common_error(),
            kind: 'error',
          }),
        )
      }
    />
  );
};

export const IPv6Switch = () => {
  const { query, upsert } = useClashConfig();

  return (
    <SwitchCard
      label={m.settings_clash_settings_ipv6_label()}
      checked={Boolean(query.data?.ipv6)}
      loading={upsert.isPending}
      onCheckedChange={(checked) =>
        void upsert.mutateAsync({ ipv6: checked }).catch((error) =>
          message(formatError(error), {
            title: m.common_error(),
            kind: 'error',
          }),
        )
      }
    />
  );
};

export const TunStackSelector = () => {
  const [coreType] = useCoreType();
  const tunStack = useSetting('tun_stack');
  const enableTun = useSetting('enable_tun_mode');
  const runtimeProfile = useRuntimeProfile();

  const options = useMemo(() => {
    const values: Record<string, string> = {
      system: 'System',
      gvisor: 'gVisor',
      mixed: 'Mixed',
    };
    if (coreType === 'clash') delete values.mixed;
    return values;
  }, [coreType]);

  const current =
    tunStack.value && tunStack.value in options ? tunStack.value : 'gvisor';

  if (coreType === 'clash-rs') return null;

  const handleSelect = async (value: string) => {
    try {
      await tunStack.upsert(value as TunStack);
      if (enableTun.value) await enableTun.upsert(true);
      await runtimeProfile.refetch();
    } catch (error) {
      message(
        `${m.settings_clash_tun_stack_change_failed()}\n${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    }
  };

  return (
    <SelectorCard
      label={m.settings_clash_settings_tun_stack_label()}
      current={current}
      options={options}
      onSelect={(value) => void handleSelect(value)}
    />
  );
};

export const LogLevelSelector = () => {
  const { query, upsert } = useClashConfig();
  const options = {
    debug: 'Debug',
    info: 'Info',
    warning: 'Warn',
    error: 'Error',
    silent: 'Silent',
  };
  const current = String(query.data?.['log-level'] ?? 'debug');

  return (
    <SelectorCard
      label={m.settings_clash_settings_log_level_label()}
      current={current}
      options={options}
      onSelect={(value) => void upsert.mutateAsync({ 'log-level': value })}
    />
  );
};

export const UWPTool = () => {
  if (getSystem() !== 'windows') return null;

  const handleOpen = async () => {
    try {
      await openUWPTool();
    } catch (error) {
      message(
        `${m.settings_clash_uwp_tool_open_failed()}\n${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    }
  };

  return (
    <SettingsCard>
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>
              {m.settings_clash_open_uwp_tool_label()}
            </ItemLabelText>
          </ItemLabel>
          <Button variant="raised" onClick={() => void handleOpen()}>
            {m.common_open()}
          </Button>
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
};
