import { useClashLogs } from '@chimera/interface';
import {
  PauseCircleOutlineRounded,
  PlayCircleOutlineRounded,
} from '@mui/icons-material';
import { IconButton, Tooltip } from '@mui/material';
import { useTranslation } from 'react-i18next';

export const LogToggle = () => {
  const { t } = useTranslation();
  const { status, disable, enable } = useClashLogs();

  const handleClick = () => {
    if (status) {
      disable();
      return;
    }

    enable();
  };

  return (
    <Tooltip title={t('Collect Logs')}>
      <IconButton size="small" color="inherit" onClick={handleClick}>
        {status ? <PauseCircleOutlineRounded /> : <PlayCircleOutlineRounded />}
      </IconButton>
    </Tooltip>
  );
};

export default LogToggle;
