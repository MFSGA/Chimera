import {
  NavigationSidebar,
  NavigationSidebarContent,
  NavigationSidebarItem,
  NavigationSidebarToggle,
} from '@chimera/ui/navigation-sidebar';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { SidebarProvider } from '@chimera/ui/slider-sidebar';
import { createFileRoute, Link, Outlet } from '@tanstack/react-router';
import ArticleRounded from '~icons/material-symbols/article-rounded';
import BugReportRounded from '~icons/material-symbols/bug-report-rounded';
import ErrorRounded from '~icons/material-symbols/error-rounded';
import HelpRounded from '~icons/material-symbols/help-rounded';
import InfoRounded from '~icons/material-symbols/info-rounded';
import SearchRounded from '~icons/material-symbols/search-rounded';
import StopCircleRounded from '~icons/material-symbols/stop-circle-rounded';
import WarningRounded from '~icons/material-symbols/warning-rounded';
import { memo, type PropsWithChildren } from 'react';
import { z } from 'zod';
import * as m from '@/paraglide/messages';
import { dataPageLayoutVariants } from '../_modules/data-page';
import { LogLevel } from './_modules/consts';

export const Route = createFileRoute('/(main)/main/logs')({
  component: RouteComponent,
  validateSearch: z.object({
    level: z.enum(LogLevel).nullable().optional(),
  }),
});

const LogLevelIcon = {
  [LogLevel.Trace]: SearchRounded,
  [LogLevel.Debug]: BugReportRounded,
  [LogLevel.Info]: InfoRounded,
  [LogLevel.Warning]: WarningRounded,
  [LogLevel.Error]: ErrorRounded,
  [LogLevel.Fatal]: StopCircleRounded,
  [LogLevel.Unknown]: HelpRounded,
} satisfies Record<LogLevel, React.FC>;

const LogLevelButton = ({
  level: inputLevel,
  children,
}: PropsWithChildren<{ level?: LogLevel }>) => {
  const { level } = Route.useSearch();
  const Icon = inputLevel ? LogLevelIcon[inputLevel] : ArticleRounded;

  return (
    <NavigationSidebarItem
      active={(inputLevel ?? null) === (level ?? null)}
      icon={<Icon className="size-6" />}
      data-slot="logs-level-filter"
      data-log-level={inputLevel ?? 'all'}
      renderLink={(content) => (
        <Link to="/main/logs" search={{ level: inputLevel }}>
          {content}
        </Link>
      )}
    >
      <span className="capitalize">{children}</span>
    </NavigationSidebarItem>
  );
};

function RouteComponent() {
  return <Outlet />;
}

// The rail owns its open state independently of incoming log updates.
const LogLevelsSidebar = memo(function LogLevelsSidebar() {
  return (
    <SidebarProvider defaultOpen={false}>
      <NavigationSidebar aria-label={m.navbar_label_logs()}>
        <ScrollArea className="min-h-0 w-full flex-1 [&>div>div]:block!">
          <NavigationSidebarContent className="pb-16">
            <LogLevelButton>All</LogLevelButton>
            {Object.values(LogLevel).map((item) => (
              <LogLevelButton key={item} level={item}>
                {item}
              </LogLevelButton>
            ))}
          </NavigationSidebarContent>
        </ScrollArea>
        <NavigationSidebarToggle expandLabel={m.navigation_sidebar_expand()}>
          {m.navigation_sidebar_collapse()}
        </NavigationSidebarToggle>
      </NavigationSidebar>
    </SidebarProvider>
  );
});

export function LogLevelsLayout({ children }: PropsWithChildren) {
  return (
    <div className={dataPageLayoutVariants()} data-slot="logs-layout">
      <LogLevelsSidebar />
      {children}
    </div>
  );
}
