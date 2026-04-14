import { useSetting } from '@chimera/interface';
import { BaseCard, BaseItem, Expand, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, TextField } from '@mui/material';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

const ProxyGuard = () => {
  const { t } = useTranslation();
  const proxyGuard = useSetting('enable_proxy_guard');

  return (
    <SwitchItem
      label={t('Proxy Guard')}
      checked={Boolean(proxyGuard.value)}
      onChange={() => proxyGuard.upsert(!proxyGuard.value)}
    />
  );
};

const ClashFieldFilter = () => {
  const { t } = useTranslation();
  const clashFields = useSetting('enable_clash_fields');

  return (
    <SwitchItem
      label={t('Enable Clash Fields Filter')}
      checked={Boolean(clashFields.value)}
      onChange={() => clashFields.upsert(!clashFields.value)}
    />
  );
};

const ProxyBypass = () => {
  const { t } = useTranslation();
  const proxyBypass = useSetting('system_proxy_bypass');

  const value = proxyBypass.value ?? '';
  const [inputValue, setInputValue] = useState(value);

  useEffect(() => {
    setInputValue(value);
  }, [value]);

  return (
    <>
      <BaseItem title={t('Proxy Bypass')}>
        <TextField
          size="small"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 220 }}
          inputProps={{ 'aria-autocomplete': 'none' }}
        />
      </BaseItem>

      <Expand open={inputValue !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => proxyBypass.upsert(inputValue)}
          >
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  );
};

const MaxLogFiles = () => {
  const { t } = useTranslation();
  const maxLogFiles = useSetting('max_log_files');

  const value = maxLogFiles.value ?? 7;
  const [inputValue, setInputValue] = useState(String(value));

  useEffect(() => {
    setInputValue(String(value));
  }, [value]);

  const parsed = Number(inputValue);
  const invalid = !Number.isInteger(parsed) || parsed < 1;
  const changed = inputValue !== String(value);

  return (
    <>
      <BaseItem title={t('Max Log Files')}>
        <TextField
          size="small"
          type="number"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          sx={{ width: 128 }}
          inputProps={{ min: 1 }}
        />
      </BaseItem>

      <Expand open={changed}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => maxLogFiles.upsert(parsed)}
            disabled={invalid}
          >
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  );
};

export const SettingClashField = () => {
  const { t } = useTranslation();

  const items = useMemo(
    () => [<ClashFieldFilter key="fields" />, <ProxyGuard key="guard" />],
    [],
  );

  return (
    <BaseCard label={t('Clash Field')}>
      <List disablePadding>
        {items}
        <ProxyBypass />
        <MaxLogFiles />
      </List>
    </BaseCard>
  );
};

export default SettingClashField;
