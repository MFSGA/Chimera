import { useProfile, useSetting } from '@chimera/interface';
import { BaseCard, BaseDialog } from '@chimera/ui';
import { Box, Typography } from '@mui/material';
import Grid from '@mui/material/Grid';
import { useMemo, useState } from 'react';
import CLASH_FIELD from '@/assets/json/clash-field.json';
import * as m from '@/paraglide/messages';
import { ClashFieldItem, LabelSwitch } from './modules/clash-field';

type ClashFieldGroup = Record<string, string>;

const FieldsControl = ({
  label,
  fields,
  enabledFields,
  onChange,
}: {
  label: string;
  fields: ClashFieldGroup;
  enabledFields?: string[];
  onChange?: (key: string) => void;
}) => {
  const [open, setOpen] = useState(false);

  // Chimera control fields are required by the app and cannot be disabled.
  const disabled = label === 'default' || label === 'handle';

  const showFields = disabled ? Object.keys(fields) : (enabledFields ?? []);

  const Item = () => {
    return Object.entries(fields).map(([fKey, fValue]) => {
      const checked = enabledFields?.includes(fKey);

      return (
        <LabelSwitch
          key={fKey}
          label={fKey}
          url={fValue}
          disabled={disabled}
          checked={disabled ? true : checked}
          onChange={onChange ? () => onChange(fKey) : undefined}
        />
      );
    });
  };

  return (
    <>
      <ClashFieldItem
        label={label}
        fields={showFields}
        onClick={() => setOpen(true)}
      />

      <BaseDialog
        title={label}
        open={open}
        close="Close"
        onClose={() => setOpen(false)}
        divider
        contentStyle={{ overflow: 'auto' }}
      >
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
          {disabled && <Typography>Clash Nyanpasu Control Fields.</Typography>}

          <Item />
        </Box>
      </BaseDialog>
    </>
  );
};

const ClashFieldSwitch = () => {
  const { value, upsert } = useSetting('enable_clash_fields');

  return (
    <LabelSwitch
      label={m.settings_clash_settings_field_filter_label()}
      checked={Boolean(value)}
      onChange={() => upsert(!value)}
    />
  );
};

export const SettingClashField = () => {
  const { query, upsert } = useProfile();

  const mergeFields = useMemo(
    () => [
      ...Object.keys(CLASH_FIELD.default),
      ...Object.keys(CLASH_FIELD.handle),
      ...(query.data?.valid ?? []),
    ],
    [query.data?.valid],
  );

  const filteredField = (fields: ClashFieldGroup): string[] => {
    const usedObjects = [];

    for (const key in fields) {
      if (
        Object.prototype.hasOwnProperty.call(fields, key) &&
        mergeFields.includes(key)
      ) {
        usedObjects.push(key);
      }
    }

    return usedObjects;
  };

  const updateFiled = async (key: string) => {
    const valid = query.data?.valid ?? [];
    const nextFields = valid.includes(key)
      ? valid.filter((item) => item !== key)
      : [...valid, key];

    await upsert.mutateAsync({ valid: nextFields });
  };

  return (
    <BaseCard label={'Clash Field'}>
      <Box sx={{ pt: 1, pb: 2 }}>
        <ClashFieldSwitch />
      </Box>

      <Grid container spacing={2}>
        {Object.entries(CLASH_FIELD).map(([key, value]) => {
          const filtered = filteredField(value);

          return (
            <FieldsControl
              key={key}
              label={key}
              fields={value}
              enabledFields={filtered}
              onChange={updateFiled}
            />
          );
        })}
      </Grid>
    </BaseCard>
  );
};

export default SettingClashField;
