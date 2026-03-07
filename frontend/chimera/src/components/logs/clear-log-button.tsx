import { useClashLogs } from '@chimera/interface';
import { Close } from '@mui/icons-material';
import { Fab, Tooltip } from '@mui/material';
import { useLockFn } from 'ahooks';
import { useTranslation } from 'react-i18next';

export const ClearLogButton = () => {
  const { t } = useTranslation();
  const { clean } = useClashLogs();

  const handleClean = useLockFn(async () => {
    await clean.mutateAsync();
  });

  return (
    <Tooltip title={t('Clear')}>
      <Fab
        color="default"
        size="medium"
        onClick={handleClean}
        sx={(theme) => ({
          position: 'absolute',
          right: 24,
          bottom: 24,
          boxShadow: 'none',
          border: `1px solid ${theme.vars.palette.divider}`,
          backgroundColor: theme.vars.palette.background.paper,
          '&:hover': {
            boxShadow: 'none',
            backgroundColor: theme.vars.palette.action.hover,
          },
        })}
      >
        <Close className="!size-7" />
      </Fab>
    </Tooltip>
  );
};

export default ClearLogButton;
