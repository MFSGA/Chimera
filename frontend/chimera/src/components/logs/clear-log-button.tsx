import { useClashLogs } from '@chimera/interface';
import { FloatingButton } from '@chimera/ui';
import { Close } from '@mui/icons-material';
import { Tooltip } from '@mui/material';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';

export const ClearLogButton = () => {
  const { clean } = useClashLogs();

  const handleClean = useLockFn(async () => {
    await clean.mutateAsync();
  });

  return (
    <Tooltip title={m.common_clear()}>
      <FloatingButton onClick={handleClean}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  );
};

export default ClearLogButton;
