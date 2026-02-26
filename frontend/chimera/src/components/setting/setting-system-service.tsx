import { BaseCard } from '@chimera/ui';
import { List } from '@mui/material';
import { useTranslation } from 'react-i18next';
import { ServerManualPromptDialogWrapper } from './modules/service-manual-prompt-dialog';
import { ServiceModeSwitch } from './modules/system-service-mode-switch';
import { ServiceStatusControl } from './modules/system-service-status-control';

export const SettingSystemService = () => {
  const { t } = useTranslation();

  return (
    <BaseCard label={t('System Service')}>
      <ServerManualPromptDialogWrapper />
      <List disablePadding>
        <ServiceModeSwitch />
        <ServiceStatusControl />
      </List>
    </BaseCard>
  );
};

export default SettingSystemService;
