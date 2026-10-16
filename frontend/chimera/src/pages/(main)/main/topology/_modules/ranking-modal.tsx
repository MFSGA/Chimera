import {
  useTrafficUsage,
  type Dimension,
  type TrafficQuery,
} from '@chimera/interface';
import { AnimatePresence } from 'motion/react';
import { useCallback, useEffect, useRef } from 'react';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
} from '@/components/ui/modal';
import { ScrollArea, useScrollAreaViewport } from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { setFilter } from '../../_modules/traffic-filters';
import Notice from './notice';
import RankingRow from './ranking-row';
import { useTrafficPage } from './traffic-context';

function LoadMore({ onVisible }: { onVisible: () => void }) {
  const { viewportRef } = useScrollAreaViewport();
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const target = end.current;
    if (!target) return;

    const observer = new IntersectionObserver(
      ([entry]) => entry.isIntersecting && onVisible(),
      { root: viewportRef.current },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, [onVisible, viewportRef]);

  return <div ref={end} aria-hidden className="h-px" />;
}

type UsageListProps = {
  query: TrafficQuery;
  dimension: Dimension;
  selected?: string;
  onSelect: (key: string) => void;
};

function UsageList({ query, dimension, selected, onSelect }: UsageListProps) {
  const { search, labelOf } = useTrafficPage();
  const { data, isError, hasNextPage, isFetchingNextPage, fetchNextPage } =
    useTrafficUsage(query, dimension, search.metric);
  const groups = data?.pages.flatMap((page) => page.groups) ?? [];
  const loadNextPage = useCallback(() => {
    if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
  }, [fetchNextPage, hasNextPage, isFetchingNextPage]);

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="flex flex-col gap-1 px-2">
        <AnimatePresence initial={false}>
          {groups.map((group, index) => (
            <RankingRow
              key={group.key}
              label={labelOf(dimension, group.key)}
              usage={group.usage}
              metric={search.metric}
              rank={index}
              active={group.key === selected}
              onSelect={() => onSelect(group.key)}
            />
          ))}
        </AnimatePresence>
        {hasNextPage && !isFetchingNextPage && (
          <LoadMore onVisible={loadNextPage} />
        )}
        {isError ? (
          <Notice error>{m.traffic_unavailable()}</Notice>
        ) : !data || isFetchingNextPage ? (
          <p className="text-on-surface-variant px-4 py-3 text-sm">
            {m.traffic_loading_more()}
          </p>
        ) : (
          groups.length === 0 && <Notice>{m.traffic_empty()}</Notice>
        )}
      </div>
    </ScrollArea>
  );
}

export default function RankingModal({
  open,
  onOpenChange,
  title,
  dimension,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  dimension: Dimension;
}) {
  const { query, search, onSearchChange } = useTrafficPage();
  const { filters } = search;
  const selected = filters.find((filter) => filter.d === dimension)?.v;
  const rankingQuery = {
    ...query,
    filters: query.filters.filter((filter) => filter.dimension !== dimension),
  };
  const handleSelect = (key: string) => {
    onSearchChange({ filters: setFilter(filters, dimension, key) });
    onOpenChange(false);
  };

  return (
    <Modal open={open} onOpenChange={onOpenChange}>
      <ModalContent>
        <div className="bg-surface text-on-surface flex max-h-[85dvh] w-[min(32rem,calc(100vw-2rem))] flex-col gap-2 rounded-3xl p-4">
          <ModalTitle className="px-4 pt-2 text-xl">{title}</ModalTitle>
          <UsageList
            query={rankingQuery}
            dimension={dimension}
            selected={selected}
            onSelect={handleSelect}
          />
          <div className="flex justify-end">
            <ModalClose>{m.common_close()}</ModalClose>
          </div>
        </div>
      </ModalContent>
    </Modal>
  );
}
