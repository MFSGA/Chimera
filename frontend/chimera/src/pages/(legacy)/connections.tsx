import { BasePage } from '@chimera/ui';
import { createFileRoute, useBlocker } from '@tanstack/react-router';
import FilterAltRounded from '~icons/material-symbols/filter-alt-rounded';
import {
  lazy,
  Suspense,
  useDeferredValue,
  useEffect,
  useRef,
  useState,
} from 'react';
import { SearchTermCtx } from '@/components/connections/connection-search-term';
import HeaderSearch from '@/components/connections/header-search';
import { Button } from '@/components/ui/button';
import * as m from '@/paraglide/messages';

const Component = lazy(
  () => import('@/components/connections/connection-page'),
);
const ColumnFilterDialog = lazy(
  () => import('@/components/connections/connections-column-filter'),
);
const ConnectionTotal = lazy(
  () => import('@/components/connections/connections-total'),
);

export const Route = createFileRoute('/(legacy)/connections')({
  component: Connections,
});

function LoadingFallback() {
  return (
    <div className="grid h-full min-h-52 place-items-center">
      <div className="border-primary/20 border-t-primary size-8 animate-spin rounded-full border-2" />
    </div>
  );
}

function Connections() {
  const [openColumnFilter, setOpenColumnFilter] = useState(false);
  const [searchTerm, setSearchTerm] = useState('');
  const deferredSearchTerm = useDeferredValue(searchTerm);

  const [mountTable, setMountTable] = useState(true);
  const deferredMountTable = useDeferredValue(mountTable);
  const viewportRef = useRef<HTMLDivElement>(null);
  const pendingNavigationRef = useRef(false);
  const { proceed } = useBlocker({
    shouldBlockFn: () => {
      if (pendingNavigationRef.current) {
        return false;
      }

      pendingNavigationRef.current = true;
      setMountTable(false);
      return true;
    },
    withResolver: true,
  });

  useEffect(() => {
    if (pendingNavigationRef.current && !deferredMountTable) {
      proceed?.();
    }
  }, [proceed, deferredMountTable]);

  return (
    <SearchTermCtx.Provider value={deferredSearchTerm}>
      <BasePage
        title={m.navbar_label_connections()}
        full
        viewportRef={viewportRef}
        header={
          <div className="flex max-h-96 w-full flex-1 items-center justify-between gap-2 pl-5">
            <Suspense fallback={null}>
              <ConnectionTotal />
            </Suspense>
            <div className="flex items-center gap-1">
              <Suspense fallback={null}>
                <ColumnFilterDialog
                  open={openColumnFilter}
                  onClose={() => setOpenColumnFilter(false)}
                />
              </Suspense>
              <HeaderSearch
                value={searchTerm}
                onChange={(event) => setSearchTerm(event.target.value)}
              />
              <Button
                icon
                variant="flat"
                aria-label={m.connections_column_filter_title()}
                onClick={() => setOpenColumnFilter(true)}
              >
                <FilterAltRounded className="size-5" />
              </Button>
            </div>
          </div>
        }
      >
        <Suspense fallback={<LoadingFallback />}>
          {mountTable && <Component viewportRef={viewportRef} />}
        </Suspense>
      </BasePage>
    </SearchTermCtx.Provider>
  );
}
