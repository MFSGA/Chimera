import { useClashLogs } from '@chimera/interface';
import { alpha } from '@chimera/ui';
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
      <IconButton
        size="small"
        color="inherit"
        onClick={handleClick}
        sx={(theme) => ({
          width: 36,
          height: 36,
          borderRadius: 10,
          backgroundColor: alpha(
            status
              ? theme.vars.palette.success.main
              : theme.vars.palette.primary.main,
            status ? 0.16 : 0.1,
          ),
          color: status
            ? theme.vars.palette.success.main
            : theme.vars.palette.text.primary,
          '&:hover': {
            backgroundColor: alpha(
              status
                ? theme.vars.palette.success.main
                : theme.vars.palette.primary.main,
              status ? 0.22 : 0.16,
            ),
          },
        })}
      >
        {status ? <PauseCircleOutlineRounded /> : <PlayCircleOutlineRounded />}
      </IconButton>
    </Tooltip>
  );
};

export default LogToggle;
