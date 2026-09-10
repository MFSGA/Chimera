import { useClashConnections } from '@chimera/interface';
import CloseRounded from '~icons/material-symbols/close-rounded';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';

export const CloseConnectionsButton = () => {
  const { deleteConnections } = useClashConnections();

  const onCloseAll = useLockFn(async () => {
    await deleteConnections.mutateAsync(undefined);
  });

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="fab"
          icon
          className="fixed right-8 bottom-8 z-10 size-16 rounded-2xl backdrop-blur"
          aria-label={m.connections_close_all_connections()}
          onClick={onCloseAll}
        >
          <CloseRounded className="size-8" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{m.connections_close_all_connections()}</TooltipContent>
    </Tooltip>
  );
};

export default CloseConnectionsButton;
