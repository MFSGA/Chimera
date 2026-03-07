import { cn } from '@chimera/ui';
import { ArticleOutlined } from '@mui/icons-material';
import { useDebounceEffect } from 'ahooks';
import { useDeferredValue, useEffect, useRef, type RefObject } from 'react';
import { useTranslation } from 'react-i18next';
import { Virtualizer, type VListHandle } from 'virtua';
import ContentDisplay from '@/components/base/content-display';
import LogItem from './log-item';
import { useLogContext } from './log-provider';

export const LogList = ({
  scrollRef,
}: {
  scrollRef: RefObject<HTMLElement>;
}) => {
  const { t } = useTranslation();
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
    const end = virtualizerRef.current?.findEndIndex() || 0;

    shouldStickToBottom.current = end + 1 === logs?.length;
  };

  const deferredLogs = useDeferredValue(logs);

  return deferredLogs?.length ? (
    <div className="h-full px-3 pt-3 pb-20 md:px-4">
      <div className="min-h-full overflow-hidden rounded-[1.75rem] border border-black/6 bg-black/[0.025] backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
        <Virtualizer
          ref={virtualizerRef}
          scrollRef={scrollRef}
          onScroll={handleScroll}
        >
          {deferredLogs.map((item, index) => {
            return (
              <LogItem
                key={`${item.time ?? 'log'}-${index}`}
                className={cn(
                  index !== 0 && 'border-t border-black/6 dark:border-white/8',
                )}
                value={item}
              />
            );
          })}
        </Virtualizer>
      </div>
    </div>
  ) : (
    <ContentDisplay className="absolute px-3 pt-3 pb-20 md:px-4">
      <div className="flex min-h-full w-full items-center justify-center">
        <div className="flex w-full max-w-md flex-col items-center rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
          <ArticleOutlined className="!mb-4 !size-14 text-zinc-400 dark:text-zinc-500" />
          <div className="text-base font-semibold text-zinc-800 dark:text-zinc-100">
            {t('No Logs')}
          </div>
          <div className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
            {t('Collect Logs')}
          </div>
        </div>
      </div>
    </ContentDisplay>
  );
};
