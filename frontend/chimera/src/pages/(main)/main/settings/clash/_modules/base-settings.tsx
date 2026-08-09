import {
  useClashConfig,
  useRuntimeProfile,
  useSetting,
  type TunStack,
} from '@chimera/interface';
import { useMemo } from 'react';
import { SelectorCard } from '@/components/settings/setting-control';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '@/components/settings/settings-card';
import { Switch } from '@/components/ui/switch';
import { useCoreType } from '@/hooks/use-store';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const AllowLanSwitch = () => {
  const { query, upsert } = useClashConfig();

  return (
    <ItemContainer data-slot="allow-lan-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_allow_lan_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
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
    </ItemContainer>
  );
};

export const IPv6Switch = () => {
  const { query, upsert } = useClashConfig();

  return (
    <ItemContainer data-slot="ipv6-switch-container">
      <ItemLabel>
        <ItemLabelText>{m.settings_clash_settings_ipv6_label()}</ItemLabelText>
      </ItemLabel>
      <Switch
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
    </ItemContainer>
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
