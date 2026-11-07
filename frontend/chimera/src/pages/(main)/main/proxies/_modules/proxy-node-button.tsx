import { type Proxy_Serialize } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { cn } from '@chimera/utils';
import FlashOnRounded from '~icons/material-symbols/flash-on-rounded';
import KeepRounded from '~icons/material-symbols/keep-rounded';
import { memo, useMemo, type ComponentProps, type MouseEvent } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import DelayChip from './delay-chip';

function FeatureChip({
  label,
  variant = 'feature',
}: {
  label: string;
  variant?: 'type' | 'feature';
}) {
  return (
    <span
      className={cn(
        'shrink-0 rounded px-1.5 py-0.5 text-[9px] leading-none font-medium uppercase',
        variant === 'type'
          ? 'bg-primary/10 text-primary'
          : 'bg-secondary-container text-on-secondary-container',
      )}
      data-slot="proxy-node-feature"
    >
      {label}
    </span>
  );
}

/** Native nodes stay plain JSON; selection and delay are separate actions. */
export default memo(function ProxyNodeButton({
  proxy,
  selectable,
  fixed = false,
  onSelect,
  onDelayTest,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children' | 'onSelect'> & {
  proxy: Proxy_Serialize;
  selectable: boolean;
  fixed?: boolean;
  onSelect: (proxy: Proxy_Serialize) => Promise<void>;
  onDelayTest: (proxy: Proxy_Serialize) => Promise<void>;
}) {
  const handleSelectProxy = useLockFn(async () => {
    if (!selectable) return;
    await onSelect(proxy);
  });

  const delayTask = useBlockTask(
    `proxy-delay-check-${proxy.name.toLowerCase()}`,
    async () => {
      await onDelayTest(proxy);
    },
  );

  const handleDelayClick = useLockFn(
    async (event: MouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      await delayTask.execute();
    },
  );

  const currentDelay = useMemo(() => {
    if (!proxy.history?.length) return -1;
    return proxy.history[proxy.history.length - 1].delay;
  }, [proxy.history]);

  return (
    <Button
      variant="fab"
      className={cn(
        'flex w-full flex-col justify-center gap-1 px-2 text-left',
        'group-data-[active=true]:bg-primary-container/75',
        'dark:group-data-[active=true]:bg-surface-variant/50',
        'group-data-[active=false]:bg-on-background/3',
        'dark:group-data-[active=false]:bg-surface/30',
        'group-data-[active=false]:shadow-none',
        'group-data-[active=false]:hover:shadow-none',
        'group-data-[active=false]:hover:bg-surface-variant/30',
        'data-[selectable=false]:cursor-default',
      )}
      data-selectable={String(selectable)}
      aria-disabled={!selectable}
      onClick={handleSelectProxy}
      {...props}
    >
      <div className="flex w-full items-center justify-between gap-2 px-2">
        <div className="truncate text-sm font-medium">{proxy.name}</div>
        {fixed && (
          <span
            className="text-primary shrink-0"
            data-slot="proxy-node-fixed-icon"
            title="Pinned proxy"
          >
            <KeepRounded className="size-4" />
          </span>
        )}
      </div>

      <div className="flex w-full items-center justify-between gap-2 overflow-hidden px-2">
        <div className="flex items-center gap-1 overflow-hidden">
          <FeatureChip label={proxy.type} variant="type" />
          {proxy.udp && <FeatureChip label="UDP" />}
          {proxy.xudp && <FeatureChip label="XUDP" />}
          {proxy.tfo && <FeatureChip label="TFO" />}
        </div>

        <Button
          className="grid h-4 min-w-10 shrink-0 place-content-center px-2 text-center"
          variant="raised"
          onClick={handleDelayClick}
          loading={delayTask.isPending}
          asChild
        >
          {currentDelay > 0 ? (
            <DelayChip delay={currentDelay} />
          ) : (
            <span>
              <FlashOnRounded className="py-1" />
            </span>
          )}
        </Button>
      </div>
    </Button>
  );
});
