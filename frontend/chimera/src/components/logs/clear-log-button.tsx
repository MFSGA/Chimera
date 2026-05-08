import { useClashLogs } from '@chimera/interface';
import { FloatingButton } from '@chimera/ui';
import { Close } from '@mui/icons-material';
import { Tooltip } from '@mui/material';
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
      <FloatingButton onClick={handleClean}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  );
};

export default ClearLogButton;
