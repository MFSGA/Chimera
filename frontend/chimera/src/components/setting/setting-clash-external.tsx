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
import { useTranslation } from 'react-i18next';

const TextItem = ({
  label,
  value,
  onApply,
}: {
  label: string;
  value: string;
  onApply: (value: string) => Promise<void>;
}) => {
  const { t } = useTranslation();
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
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  );
};

const ExternalController = () => {
  const { t } = useTranslation();
  const { data, refetch } = useClashInfo();
  const { upsert } = useClashCoreConfig();
  const runtimeProfile = useRuntimeProfile();

  return (
    <TextItem
      label={t('External Controller')}
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
  const { t } = useTranslation();
  const { value, upsert } = useSetting('clash_strategy');

  const portStrategyOptions = {
    allow_fallback: t('Allow Fallback'),
    fixed: t('Fixed'),
    random: t('Random'),
  };

  const selected = useMemo(
    () => value?.external_controller_port_strategy || 'allow_fallback',
    [value],
  );

  return (
    <MenuItem
      label={t('Port Strategy')}
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
  const { t } = useTranslation();
  const { data, refetch } = useClashInfo();
  const { upsert } = useClashCoreConfig();

  return (
    <TextItem
      label={t('Core Secret')}
      value={data?.secret || ''}
      onApply={async (value) => {
        await upsert.mutateAsync({ secret: value });
        await refetch();
      }}
    />
  );
};

export const SettingClashExternal = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('Clash External Controll')}>
      <List disablePadding>
        <ExternalController />
        <PortStrategy />
        <CoreSecret />
      </List>
    </BaseCard>
  );
};

export default SettingClashExternal;
