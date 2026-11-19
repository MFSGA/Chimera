import type {
  DelayHistory as DelayHistorySample,
  Proxy_Serialize,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import HighlightText from '@chimera/ui/highlight-text';
import { cn } from '@chimera/utils';
import FlashOnRounded from '~icons/material-symbols/flash-on-rounded';
import KeepRounded from '~icons/material-symbols/keep-rounded';
import { memo, type ComponentProps, type MouseEvent } from 'react';
import { Notice } from '@/components/base';
import { useBlockTask } from '@/components/providers/block-task-provider';
import DelayHistory from '@/components/proxies/delay-history';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
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
        'shrink-0 rounded-full px-1.5 py-0.5 text-[9px] leading-none font-medium uppercase',
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
  history,
  delay,
  leaf,
  selectable,
  fixed = false,
  searchText = '',
  onSelect,
  onDelayTest,
  ...props
}: Omit<ComponentProps<typeof Button>, 'onClick' | 'children' | 'onSelect'> & {
  proxy: Proxy_Serialize;
  /** Samples for this group's resolved leaf and health check source. */
  history: DelayHistorySample[];
  /** Nested group's final selected node, when one exists. */
  leaf?: string;
  /** Last result from this group's opaque delay source. */
  delay?: number;
  selectable: boolean;
  fixed?: boolean;
  /** Highlight only a single search term, as in ref. */
  searchText?: string;
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
      try {
        await delayTask.execute();
      } catch {
        // Use the ref translation rather than leaving a rejected async
        // click handler and no visible feedback after a failed native test.
        Notice.error(m.error_clash_read_delay_test_failed(), 5000);
      }
    },
  );

  const currentDelay = delay ?? -1;

  return (
    <DelayHistory history={history}>
      <Button
        variant="fab"
        className={cn(
          'flex w-full flex-col justify-center gap-1 px-2 text-left',
          // Ref uses a background tint instead of the fab brightness filter:
          // an opacity compositor layer would jitter the nested chips.
          'isolate hover:filter-none!',
          'before:absolute before:inset-0 before:-z-10 before:transition-colors',
          'hover:before:bg-on-surface/5',
          'group-data-[active=true]:bg-primary-container/75',
          'dark:group-data-[active=true]:bg-surface-variant/50',
          'group-data-[active=false]:bg-on-background/3',
          'dark:group-data-[active=false]:bg-surface/30',
          'group-data-[active=false]:shadow-none',
          'group-data-[active=false]:hover:shadow-none',
          'group-data-[active=false]:hover:bg-surface-variant/30',
          'data-[selectable=false]:cursor-default',
          'data-[selectable=false]:hover:before:bg-transparent',
        )}
        data-slot="proxy-node-card"
        data-proxy-name={proxy.name}
        data-selectable={String(selectable)}
        aria-disabled={!selectable}
        onClick={handleSelectProxy}
        {...props}
      >
        <div className="flex w-full items-center justify-between gap-2 px-2">
          <div className="truncate text-sm font-medium">
            <HighlightText searchText={searchText}>{proxy.name}</HighlightText>
          </div>
          {fixed && (
            <span
              className="text-primary shrink-0"
              data-slot="proxy-node-fixed-icon"
              title={m.proxies_group_fixed_label()}
            >
              <KeepRounded className="size-4" />
            </span>
          )}
        </div>

        <div className="flex w-full items-center justify-between gap-2 overflow-hidden px-2">
          <div
            className="flex items-center gap-1 overflow-hidden"
            data-slot="proxy-node-features"
          >
            <FeatureChip label={proxy.type} variant="type" />
            {proxy.udp && <FeatureChip label="UDP" />}
            {proxy.xudp && <FeatureChip label="XUDP" />}
            {proxy.tfo && <FeatureChip label="TFO" />}
          </div>

          {leaf && (
            <span
              className="text-on-surface-variant min-w-0 flex-1 truncate text-xs"
              title={leaf}
              data-slot="proxy-node-leaf"
            >
              → {leaf}
            </span>
          )}

          <Button
            className="grid h-4 min-w-10 shrink-0 place-content-center px-2 text-center"
            variant="raised"
            data-slot="proxy-node-delay-test-trigger"
            data-error={String(delayTask.isError)}
            data-success={String(delayTask.isSuccess)}
            aria-label={m.proxies_group_delay_test_title()}
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
    </DelayHistory>
  );
});
