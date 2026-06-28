import { cn } from '@chimera/ui';
import { useDebounceEffect } from 'ahooks';
import { useDeferredValue, useEffect, useRef, type RefObject } from 'react';
import { Virtualizer, type VListHandle } from 'virtua';
import ContentDisplay from '@/components/base/content-display';
import * as m from '@/paraglide/messages';
import LogItem from './log-item';
import { useLogContext } from './log-provider';

export const LogList = ({
  scrollRef,
}: {
  scrollRef: RefObject<HTMLElement>;
}) => {
  const { logs, logLevel } = useLogContext();
  const virtualizerRef = useRef<VListHandle>(null);
  const shouldStickToBottom = useRef(true);
  const isFirstScroll = useRef(true);

  useDebounceEffect(
    () => {
      if (shouldStickToBottom.current && logs?.length) {
        virtualizerRef.current?.scrollToIndex(logs.length - 1, {
          align: 'end',
          smooth: !isFirstScroll.current,
        });

        isFirstScroll.current = false;
      }
    },
    [logs],
    { wait: 100 },
  );

  useEffect(() => {
    isFirstScroll.current = true;
  }, [logLevel]);

  const handleScroll = () => {
    // todo: fix ih the future
    // const end = virtualizerRef.current?.findEndIndex() || 0;
    const end = 0;

    shouldStickToBottom.current = end + 1 === logs?.length;
  };

  const deferredLogs = useDeferredValue(logs);

  return deferredLogs?.length ? (
    <Virtualizer
      ref={virtualizerRef}
      scrollRef={scrollRef}
      onScroll={handleScroll}
    >
      {deferredLogs.map((item, index) => {
        return (
          <LogItem
            key={index}
            className={cn(index !== 0 && 'border-t border-zinc-500')}
            value={item}
          />
        );
      })}
    </Virtualizer>
  ) : (
    <ContentDisplay className="absolute" message={m.logs_empty_message()} />
  );
};
