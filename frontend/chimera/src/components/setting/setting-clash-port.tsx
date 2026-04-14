import { useClashConfig, useClashCoreConfig, useSetting } from '@chimera/interface';
import { BaseCard, BaseItem, Expand } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, TextField } from '@mui/material';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { message } from '@/utils/notification';

const MIXED_PORT_FALLBACK = 7890;

const ClashPort = () => {
  const { t } = useTranslation();
  const { value, upsert } = useSetting('verge_mixed_port');
  const { query } = useClashConfig();
  const { upsert: upsertClashCore } = useClashCoreConfig();

  const port = useMemo(
    () => query.data?.['mixed-port'] || value || MIXED_PORT_FALLBACK,
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
      message(t('Port must be between 1 and 65535.'), {
        title: t('Error'),
        kind: 'error',
      });
      return;
    }

    await upsertClashCore.mutateAsync({ 'mixed-port': parsed });
    await upsert(parsed);
  };

  return (
    <>
      <BaseItem title={t('Mixed Port')}>
        <TextField
          size="small"
          type="number"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 128 }}
          inputProps={{ min: 1, max: 65535 }}
        />
      </BaseItem>

      <Expand open={changed}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={handleApply}
          >
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  );
};

export const SettingClashPort = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('Clash Port')}>
      <List disablePadding>
        <ClashPort />
      </List>
    </BaseCard>
  );
};

export default SettingClashPort;
