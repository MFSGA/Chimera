import { BasePage } from '@chimera/ui';
import { FilterAlt } from '@mui/icons-material';
import { Box, CircularProgress, IconButton } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useThrottle } from 'ahooks';
import { lazy, Suspense, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { SearchTermCtx } from '@/components/connections/connection-search-term';
import HeaderSearch from '@/components/connections/header-search';

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

function Connections() {
  const { t } = useTranslation();

  const [openColumnFilter, setOpenColumnFilter] = useState(false);

  const [searchTerm, setSearchTerm] = useState<string>();
  const throttledSearchTerm = useThrottle(searchTerm, { wait: 150 });

  // Loading fallback component
  const LoadingFallback = () => (
    <Box
      sx={{
        display: 'flex',
        justifyContent: 'center',
        alignItems: 'center',
        height: '100%',
        minHeight: 200,
      }}
    >
      <CircularProgress />
    </Box>
  );

  return (
    <SearchTermCtx.Provider value={throttledSearchTerm}>
      <BasePage
        title={t('Connections')}
        full
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
                onChange={(e) => setSearchTerm(e.target.value)}
              />
              <IconButton onClick={() => setOpenColumnFilter(true)}>
                <FilterAlt />
              </IconButton>
            </div>
          </div>
        }
      >
        <Suspense fallback={<LoadingFallback />}>
          <Component />
        </Suspense>
      </BasePage>
    </SearchTermCtx.Provider>
  );
}
