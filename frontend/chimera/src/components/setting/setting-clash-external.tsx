import {
  useClashCoreConfig,
  useClashInfo,
  useRuntimeProfile,
  useSetting,
  type ExternalControllerPortStrategy,
} from '@chimera/interface';
import { BaseCard, BaseItem, Expand, MenuItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, TextField } from '@mui/material';
import { useEffect, useMemo, useState } from 'react';
import * as m from '@/paraglide/messages';

const TextItem = ({
  label,
  value,
  onApply,
}: {
  label: string;
  value: string;
  onApply: (value: string) => Promise<void>;
}) => {
  const [textValue, setTextValue] = useState(value);

  useEffect(() => {
    setTextValue(value);
  }, [value]);

  return (
    <>
      <BaseItem title={label}>
        <TextField
          size="small"
          value={textValue}
          onChange={(e) => setTextValue(e.target.value)}
          sx={{ width: 160 }}
          slotProps={{ htmlInput: { 'aria-autocomplete': 'none' } }}
        />
      </BaseItem>

      <Expand open={textValue !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => onApply(textValue)}
          >
            {m.common_apply()}
          </Button>
        </div>
      </Expand>
    </>
  );
};

const ExternalController = () => {
  const { data, refetch } = useClashInfo();
  const { upsert } = useClashCoreConfig();
  const runtimeProfile = useRuntimeProfile();

  return (
    <TextItem
      label={m.settings_clash_settings_external_controll_label()}
      value={data?.server || ''}
      onApply={async (value) => {
        await upsert.mutateAsync({ 'external-controller': value });
        await refetch();
        await new Promise((resolve) => setTimeout(resolve, 300));
        await runtimeProfile.refetch();
      }}
    />
  );
};

const PortStrategy = () => {
  const { value, upsert } = useSetting('clash_strategy');

  const portStrategyOptions = {
    allow_fallback: m.settings_clash_settings_allow_fallback_label(),
    fixed: m.settings_clash_settings_fixed_label(),
    random: m.settings_clash_settings_random_label(),
  };

  const selected = useMemo(
    () => value?.external_controller_port_strategy || 'allow_fallback',
    [value],
  );

  return (
    <MenuItem
      label={m.settings_clash_settings_port_strategy_label()}
      options={portStrategyOptions}
      selected={selected}
      selectSx={{ width: 160 }}
      onSelected={async (nextValue) => {
        await upsert({
          external_controller_port_strategy:
            nextValue as ExternalControllerPortStrategy,
        });
      }}
    />
  );
};

const CoreSecret = () => {
  const { data, refetch } = useClashInfo();
  const { upsert } = useClashCoreConfig();

  return (
    <TextItem
      label={m.settings_clash_settings_core_secret_label()}
      value={data?.secret || ''}
      onApply={async (value) => {
        await upsert.mutateAsync({ secret: value });
        await refetch();
      }}
    />
  );
};

export const SettingClashExternal = () => {
  return (
    <BaseCard label={m.settings_label_external_controll()}>
      <List disablePadding>
        <ExternalController />
        <PortStrategy />
        <CoreSecret />
      </List>
    </BaseCard>
  );
};

export default SettingClashExternal;
