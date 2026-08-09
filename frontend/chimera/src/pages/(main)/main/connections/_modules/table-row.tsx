import { useClashConnections } from '@chimera/interface';
import { cn } from '@chimera/ui';
import ChatInfoRounded from '~icons/material-symbols/chat-info-rounded';
import CloseRounded from '~icons/material-symbols/close-rounded';
import { sentenceCase } from 'change-case';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import { useState, type ComponentProps } from 'react';
import {
  RegisterContextMenu,
  RegisterContextMenuContent,
  RegisterContextMenuTrigger,
} from '@/components/providers/context-menu-provider';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { ContextMenuItem } from '@/components/ui/context-menu';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
} from '@/components/ui/modal';
import { ScrollArea } from '@/components/ui/scroll-area';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import type { ConnectionRow } from '..';

const INTERNAL_KEYS = new Set(['closed', 'downloadSpeed', 'uploadSpeed']);

/** Format connection detail values using the same units as the table. */
function formatValue(key: string, value: unknown): React.ReactNode {
  if (Array.isArray(value)) {
    return <span>{value.join(' / ')}</span>;
  }

  const normalizedKey = key.toLowerCase();

  if (normalizedKey.includes('speed')) {
    return <span>{filesize(value as number, { standard: 'iec' })}/s</span>;
  }

  if (normalizedKey.includes('download') || normalizedKey.includes('upload')) {
    return <span>{filesize(value as number, { standard: 'iec' })}</span>;
  }

  if (
    normalizedKey.includes('port') ||
    normalizedKey === 'id' ||
    normalizedKey.includes('ip')
  ) {
    return <span>{String(value)}</span>;
  }

  const date = dayjs(value as string | number | Date | null | undefined);

  if (date.isValid() && typeof value === 'string' && value.includes('T')) {
    return (
      <span title={date.format('YYYY-MM-DD HH:mm:ss')}>{date.fromNow()}</span>
    );
  }

  return <span>{String(value)}</span>;
}

/** Render a label/value pair in the connection details grid. */
function RowRender({ label, value }: { label: string; value: unknown }) {
  const key = label.toLowerCase();

  return (
    <>
      <div className="w-fit text-sm font-semibold">{sentenceCase(label)}</div>
      <div
        className={cn(
          'text-sm break-all',
          (key === 'id' ||
            key.includes('ip') ||
            key.includes('port') ||
            key.includes('destination') ||
            key.includes('path')) &&
            'font-mono',
        )}
      >
        {formatValue(key, value)}
      </div>
    </>
  );
}

/** Render a virtualized connection row with ref-style context menu and modal. */
export default function TableRow({
  data,
  onDoubleClick,
  ...props
}: ComponentProps<'tr'> & {
  data: ConnectionRow;
}) {
  const { deleteConnections } = useClashConnections();
  const [open, setOpen] = useState(false);

  const handleCloseConnection = useLockFn(async () => {
    if (open) {
      setOpen(false);
    }

    await deleteConnections.mutateAsync(data.id);
  });

  return (
    <>
      <RegisterContextMenu>
        <RegisterContextMenuTrigger asChild>
          <tr
            onDoubleClick={(event) => {
              onDoubleClick?.(event);
              setOpen(true);
            }}
            {...props}
          />
        </RegisterContextMenuTrigger>

        <RegisterContextMenuContent>
          <ContextMenuItem onSelect={() => setOpen(true)}>
            <ChatInfoRounded className="size-4" />
            <span>{m.connections_view_details()}</span>
          </ContextMenuItem>

          <ContextMenuItem onSelect={() => handleCloseConnection()}>
            <CloseRounded className="size-4" />
            <span>{m.connections_close_connection()}</span>
          </ContextMenuItem>
        </RegisterContextMenuContent>
      </RegisterContextMenu>

      <Modal open={open} onOpenChange={setOpen}>
        <ModalContent>
          <Card divider className="flex max-w-[80vw] min-w-96 flex-col">
            <CardHeader>
              <ModalTitle>{m.connections_view_details()}</ModalTitle>
            </CardHeader>

            <CardContent asChild className="p-0">
              <ScrollArea className="max-h-[70vh] select-text">
                <div className="grid grid-cols-[max-content_1fr] gap-x-4 gap-y-2 p-4">
                  {Object.entries(data)
                    .filter(
                      ([key, value]) =>
                        key !== 'metadata' &&
                        !INTERNAL_KEYS.has(key) &&
                        value !== undefined &&
                        value !== null &&
                        value !== '',
                    )
                    .map(([key, value]) => (
                      <RowRender key={key} label={key} value={value} />
                    ))}

                  <h3 className="col-span-2 pt-4 pb-1 text-base font-semibold">
                    Metadata
                  </h3>

                  {Object.entries(data.metadata)
                    .filter(
                      ([, value]) =>
                        value !== undefined && value !== null && value !== '',
                    )
                    .map(([key, value]) => (
                      <RowRender key={key} label={key} value={value} />
                    ))}
                </div>
              </ScrollArea>
            </CardContent>

            <CardFooter className="gap-2">
              <ModalClose variant="flat">{m.common_close()}</ModalClose>

              <Button onClick={handleCloseConnection}>
                {m.connections_close_connection()}
              </Button>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </>
  );
}
