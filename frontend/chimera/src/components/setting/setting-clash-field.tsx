import { useSetting } from '@chimera/interface';
import { BaseCard, SwitchItem } from '@chimera/ui';
import { List } from '@mui/material';
import { useTranslation } from 'react-i18next';

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

export const SettingClashField = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('Clash Field')}>
      <List disablePadding>
        <ClashFieldFilter />
      </List>
    </BaseCard>
  );
};

export default SettingClashField;
