import { createFileRoute } from '@tanstack/react-router';
import { useCallback } from 'react';
import { keepReturn } from '@/components/router/cross-navigation';
import { ReturnButton } from '@/components/router/return-button';
import {
  useCrossNavigate,
  useEntryFocus,
} from '@/components/router/use-cross-navigate';
import ConnectionsContent, {
  type ConnectionsViewState,
} from './_modules/connections-content';
import type { ConnectionDetail } from './_modules/table-row';
import { Route as ConnectionsRoute } from './route';

export const Route = createFileRoute('/(main)/main/connections/')({
  component: RouteComponent,
});

function RouteComponent() {
  const params = ConnectionsRoute.useSearch();
  const navigate = ConnectionsRoute.useNavigate();
  const crossNavigate = useCrossNavigate();
  const entryFocus = useEntryFocus();

  const onChange = useCallback(
    (patch: Partial<ConnectionsViewState>) =>
      navigate({
        search: (previous) => ({
          ...previous,
          ...patch,
          filters: patch.filters?.length
            ? patch.filters
            : patch.filters === undefined
              ? previous.filters
              : undefined,
        }),
        replace: true,
        state: keepReturn,
      }),
    [navigate],
  );

  const onLocateRule = useCallback(
    (detail: ConnectionDetail) =>
      crossNavigate({
        from: 'connections',
        originFocus: detail.rowId,
        targetFocus: detail.ruleLabel,
        to: { to: '/main/rules', search: {} },
      }),
    [crossNavigate],
  );

  return (
    <ConnectionsContent
      value={{
        ...params,
        scope: params.scope ?? 'active',
        filters: params.filters ?? [],
      }}
      onChange={onChange}
      start={<ReturnButton />}
      onLocateRule={onLocateRule}
      entryFocus={entryFocus}
    />
  );
}
