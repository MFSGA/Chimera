import { useClashConfig, useClashSetting } from '@chimera/interface';
import { BaseCard, BaseItem, Expand, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, TextField } from '@mui/material';
import { useEffect, useMemo, useState } from 'react';
import * as m from '@/paraglide/messages';
import { message } from '@/utils/notification';

const MIXED_PORT_FALLBACK = 7890;

const ClashPort = () => {
  const { value, upsert } = useClashSetting('mixed_port');
  const { query } = useClashConfig();

  const port = useMemo(
    () =>
      query.data?.['mixed-port'] || value?.start_port || MIXED_PORT_FALLBACK,
    [query.data, value],
  );

  const [inputValue, setInputValue] = useState(String(port));

  useEffect(() => {
    setInputValue(String(port));
  }, [port]);

  const parsed = Number(inputValue);
  const invalid = !Number.isInteger(parsed) || parsed < 1 || parsed > 65535;
  const changed = inputValue !== String(port);

  const handleApply = async () => {
    if (invalid) {
      message(m.settings_clash_port_validation_error(), {
        title: m.common_error(),
        kind: 'error',
      });
      return;
    }

    await upsert({ start_port: parsed });
  };

  return (
    <>
      <BaseItem title={m.settings_clash_settings_mixed_port_label()}>
        <TextField
          size="small"
          type="number"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 128 }}
          slotProps={{ htmlInput: { min: 1, max: 65535 } }}
        />
      </BaseItem>

      <Expand open={changed}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={handleApply}
          >
            {m.common_apply()}
          </Button>
        </div>
      </Expand>
    </>
  );
};

const RandomPort = () => {
  const { value, upsert } = useClashSetting('mixed_port');

  const handleRandomPort = async () => {
    try {
      await upsert({ kind: value?.kind === 'random' ? 'fixed' : 'random' });
    } catch (e) {
      message(JSON.stringify(e), {
        title: m.common_error(),
        kind: 'error',
      });
    } finally {
      message(m.settings_clash_port_restart_to_effect(), {
        title: m.common_success(),
        kind: 'info',
      });
    }
  };

  return (
    <SwitchItem
      label={m.settings_clash_settings_random_port_label()}
      checked={value?.kind === 'random'}
      onChange={handleRandomPort}
    />
  );
};

export const SettingClashPort = () => {
  return (
    <BaseCard label={m.settings_clash_settings_port_label()}>
      <List disablePadding>
        <ClashPort />

        <RandomPort />
      </List>
    </BaseCard>
  );
};

export default SettingClashPort;
