import { GlassControlGroup } from '@chimera/ui/glass-control-group';
import type { ReactNode } from 'react';
import * as m from '@/paraglide/messages';
import {
  DataPage,
  DataPageSurface,
  DataPageToolbar,
} from '../../_modules/data-page';
import { LogLevelsLayout } from '../route';

/**
 * The shared log page shell follows ref's LogsLayout. Source switching is
 * intentionally deferred until the file-log and core-log RPCs are available.
 */
export function LogsLayout({
  search,
  actions,
  children,
}: {
  search: ReactNode;
  actions: ReactNode;
  children: ReactNode;
}) {
  return (
    <LogLevelsLayout>
      <DataPage
        title={m.navbar_label_logs()}
        toolbar={
          <DataPageToolbar data-slot="logs-toolbar">
            {search}
            <GlassControlGroup
              className="max-w-full flex-wrap"
              data-slot="logs-actions"
            >
              {actions}
            </GlassControlGroup>
          </DataPageToolbar>
        }
      >
        <DataPageSurface>{children}</DataPageSurface>
      </DataPage>
    </LogLevelsLayout>
  );
}
