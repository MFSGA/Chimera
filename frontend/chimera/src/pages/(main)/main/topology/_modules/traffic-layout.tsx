import {
  useSetting,
  useTrafficReport,
  type ReportRequest,
} from '@chimera/interface';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { Link } from '@tanstack/react-router';
import {
  useMemo,
  useState,
  type PropsWithChildren,
  type ReactNode,
} from 'react';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { keepReturn } from '@/components/router/cross-navigation';
import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@/components/ui/navigation-sidebar';
import { SidebarProvider } from '@/components/ui/slider-sidebar';
import * as m from '@/paraglide/messages';
import { usageAmount } from '@/utils/traffic-usage';
import {
  DataPage,
  dataPageLayoutVariants,
  DataPageSurface,
  useDataPageBottomInset,
} from '../../_modules/data-page';
import { useUsageLabelOf } from '../../_modules/use-usage-label-of';
import Notice from './notice';
import {
  pollInterval,
  toQuery,
  toTopologyRequest,
  type TrafficSearch,
} from './search';
import {
  RANKINGS,
  TRAFFIC_PAGES,
  TrafficPageContext,
  withoutTrafficView,
  type TrafficView,
} from './traffic-context';
import TrafficToolbar from './traffic-toolbar';
import { TrafficViewSettingsProvider } from './view-settings';

/** The scroll content stays above the floating toolbar as the page grows. */
function TrafficSurface({ children }: PropsWithChildren) {
  const bottomInset = useDataPageBottomInset();

  return (
    <DataPageSurface>
      <ScrollArea className="min-h-0 flex-1 overflow-clip [&>[data-slot=scroll-area-viewport]>div]:flex! [&>[data-slot=scroll-area-viewport]>div]:min-h-full! [&>[data-slot=scroll-area-viewport]>div]:flex-col">
        <div
          className="@container relative mx-auto flex min-h-full w-full max-w-7xl flex-none flex-col gap-3 p-3 md:p-4"
          data-slot="traffic-content"
          style={{ paddingBottom: bottomInset }}
        >
          {children}
        </div>
      </ScrollArea>
    </DataPageSurface>
  );
}

export default function TrafficLayout({
  view,
  search,
  onSearchChange,
  onViewConnections,
  toolbarStart,
}: {
  view: TrafficView;
  search: TrafficSearch;
  onSearchChange: (update: Partial<TrafficSearch>) => void;
  onViewConnections?: () => void;
  toolbarStart?: ReactNode;
}) {
  const [paused, setPaused] = useState(false);
  const { value: retention } = useSetting('traffic_retention');
  const labelOf = useUsageLabelOf();
  const { range, scope, filters, layers, metric, limit } = search;

  const query = useMemo(
    () => toQuery({ range, scope, filters }),
    [range, scope, filters],
  );
  const request = useMemo<ReportRequest>(
    () => ({
      query,
      metric,
      rankings: RANKINGS.map(({ dimension }) => dimension),
      ranking_limit: 5,
      topology: toTopologyRequest({ view, layers, limit }),
    }),
    [query, metric, view, layers, limit],
  );

  // Geography uses only region data already present in the traffic report.
  // Local-IP probing is intentionally not enabled by this UI migration.
  const { data: report, isError } = useTrafficReport(request, {
    refetchInterval: paused ? false : pollInterval(range),
  });
  const empty =
    !!report &&
    report.total.connections === 0 &&
    report.total.bytes.upload + report.total.bytes.download === 0;
  const title = TRAFFIC_PAGES.find((page) => page.view === view)!.title();

  return (
    <TrafficPageContext
      value={{
        search,
        report,
        query,
        labelOf,
        onSearchChange,
        empty,
        retention: retention ?? undefined,
        paused,
        onPausedChange: setPaused,
        onViewConnections,
      }}
    >
      <TrafficViewSettingsProvider>
        <SidebarProvider defaultOpen={false}>
          <div className={dataPageLayoutVariants()} data-slot="traffic-page">
            <NavigationSidebar aria-label={m.traffic_sidebar()}>
              <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
                <NavigationSidebarContent className="pb-16">
                  {TRAFFIC_PAGES.map(
                    ({ view: item, to, icon: Icon, title: itemTitle }) => (
                      <NavigationSidebarItem
                        key={item}
                        active={view === item}
                        icon={<Icon />}
                        data-slot="traffic-navigation-item"
                        renderLink={(content) => (
                          <Link
                            to={to as never}
                            search={withoutTrafficView as never}
                            state={keepReturn as never}
                          >
                            {content}
                          </Link>
                        )}
                      >
                        {itemTitle()}
                      </NavigationSidebarItem>
                    ),
                  )}
                </NavigationSidebarContent>
              </ScrollArea>
              <NavigationSidebarToggle
                expandLabel={m.navigation_sidebar_expand()}
              >
                {m.navigation_sidebar_collapse()}
              </NavigationSidebarToggle>
            </NavigationSidebar>
            <DataPage
              title={m.topology_title()}
              toolbar={<TrafficToolbar>{toolbarStart}</TrafficToolbar>}
            >
              <TrafficSurface>
                <div className="flex flex-wrap items-center justify-between gap-2 px-1">
                  <h1 className="m-0! text-lg! font-medium">{title}</h1>
                  {view !== 'overview' && report && !isError && (
                    <p className="text-on-surface-variant text-xs tabular-nums">
                      {usageAmount(report.total, 'bytes')} ·{' '}
                      {usageAmount(report.total, 'connections')}
                    </p>
                  )}
                </div>
                {isError ? (
                  <Notice error>{m.traffic_unavailable()}</Notice>
                ) : (
                  <AnimatedOutletPreset className="relative flex min-w-0 flex-1 flex-col gap-3" />
                )}
              </TrafficSurface>
            </DataPage>
          </div>
        </SidebarProvider>
      </TrafficViewSettingsProvider>
    </TrafficPageContext>
  );
}
