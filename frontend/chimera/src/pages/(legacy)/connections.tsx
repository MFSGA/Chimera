import { BasePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useCallback, useState } from 'react';
import ContextMenuProvider from '@/components/providers/context-menu-provider';
import ConnectionsContent, {
  type ConnectionsViewState,
} from '@/pages/(main)/main/connections/_modules/connections-content';
import type { ConnectionDetail } from '@/pages/(main)/main/connections/_modules/table-row';
import * as m from '@/paraglide/messages';
import './connections.scss';

export const Route = createFileRoute('/(legacy)/connections')({
  component: Connections,
});

function Connections() {
  const navigate = Route.useNavigate();
  const [value, setValue] = useState<ConnectionsViewState>({
    scope: 'active',
    filters: [],
  });
  const onChange = useCallback((patch: Partial<ConnectionsViewState>) => {
    setValue((previous) => ({ ...previous, ...patch }));
  }, []);

  const onLocateRule = (detail: ConnectionDetail) =>
    navigate({ to: '/rules', search: { q: detail.ruleLabel } });

  return (
    <div className="legacy-connections-page h-full min-h-0">
      <ContextMenuProvider>
        <div className="h-full min-h-0">
          <BasePage title={m.navbar_label_connections()} full>
            <ConnectionsContent
              value={value}
              onChange={onChange}
              onLocateRule={onLocateRule}
            />
          </BasePage>
        </div>
      </ContextMenuProvider>
    </div>
  );
}
