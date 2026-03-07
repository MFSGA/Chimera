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
        sx={{ position: 'absolute', right: 24, bottom: 24 }}
      >
        <Close />
      </Fab>
    </Tooltip>
  );
};

export default ClearLogButton;
