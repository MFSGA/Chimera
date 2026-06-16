import { useClashConnections } from '@chimera/interface';
import { FloatingButton } from '@chimera/ui';
import { Close } from '@mui/icons-material';
import { Tooltip } from '@mui/material';
import { useLockFn } from 'ahooks';
import * as m from '@/paraglide/messages';

export const CloseConnectionsButton = () => {
  const { deleteConnections } = useClashConnections();

  const onCloseAll = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined);
  });

  return (
    <Tooltip title={m.connections_close_all_connections()}>
      <FloatingButton onClick={onCloseAll}>
        <Close className="absolute !size-8" />
      </FloatingButton>
    </Tooltip>
  );
};

export default CloseConnectionsButton;
