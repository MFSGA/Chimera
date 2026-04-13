import { useClashConnections } from '@chimera/interface';
import { Close } from '@mui/icons-material';
import { Fab, Tooltip } from '@mui/material';
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
      <Fab
        color="default"
        className="!fixed right-8 bottom-8 z-10"
        onClick={onCloseAll}
      >
        <Close className="!size-8" />
      </Fab>
    </Tooltip>
  );
};

export default CloseConnectionsButton;
