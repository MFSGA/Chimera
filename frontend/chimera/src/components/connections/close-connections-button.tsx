import { useClashConnections } from '@chimera/interface';
import { FloatingButton } from '@chimera/ui';
import { Close } from '@mui/icons-material';
import { Tooltip } from '@mui/material';
import { useLockFn } from 'ahooks';
import { useTranslation } from 'react-i18next';

export const CloseConnectionsButton = () => {
  const { t } = useTranslation();
  const { deleteConnections } = useClashConnections();

  const onCloseAll = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined);
  });

  return (
    <Tooltip title={t('Close All')}>
      <FloatingButton onClick={onCloseAll}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  );
};

export default CloseConnectionsButton;
