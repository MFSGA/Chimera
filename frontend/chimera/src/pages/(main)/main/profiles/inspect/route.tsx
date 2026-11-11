import { queries, unwrapQueryOptions, useProfile } from '@chimera/interface';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useMemo, useState } from 'react';
import { z } from 'zod';
import * as m from '@/paraglide/messages';
import {
  buildSteps,
  filterSteps,
  matchField,
  resolveStep,
} from './_modules/inspect-steps';
import InspectToolbar, { type SnapshotView } from './_modules/inspect-toolbar';
import StepDetail from './_modules/step-detail';
import StepRail from './_modules/step-rail';

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
  validateSearch: z.object({ step: z.string().optional() }),
});

function RouteComponent() {
  const client = useQueryClient();
  const navigate = useNavigate({ from: Route.fullPath });
  const { step: selectedKey } = Route.useSearch();
  const { query: profilesQuery } = useProfile();
  const [view, setView] = useState<SnapshotView>('latest');
  const [fieldQuery, setFieldQuery] = useState('');
  const [onlyChanged, setOnlyChanged] = useState(false);

  const query =
    view === 'applied'
      ? queries.inspectAppliedRuntime()
      : queries.inspectRuntime();
  const inspection = useQuery({
    ...unwrapQueryOptions(query, query.queryFn!),
    retry: false,
    refetchOnWindowFocus: false,
  });
  const snapshot = inspection.data;

  const profiles = useMemo(
    () =>
      new Map(
        (profilesQuery.data?.items ?? []).map((item) => [item.uid, item.name]),
      ),
    [profilesQuery.data?.items],
  );
  const steps = useMemo(() => {
    if (!snapshot) return [];
    const final = snapshot.final_id;
    return final === undefined ? [] : buildSteps(snapshot.nodes, final);
  }, [snapshot]);
  const selected = resolveStep(steps, selectedKey);
  const visibleSteps = filterSteps(steps, {
    onlyChanged,
    selectedKey: selected?.key,
  });

  const matchedKeys = useMemo(
    () =>
      fieldQuery.trim()
        ? new Set(
            steps
              .filter((step) => matchField(step, fieldQuery))
              .map((step) => step.key),
          )
        : undefined,
    [steps, fieldQuery],
  );

  const selectStep = (key: string) => {
    void navigate({
      to: '.',
      search: (previous) => ({ ...previous, step: key }),
      replace: true,
    });
  };

  const cycleMatch = (delta: 1 | -1) => {
    if (!selected || !matchedKeys || matchedKeys.size === 0) return;
    const matches = steps.filter((step) => matchedKeys.has(step.key));
    const current = matches.findIndex((step) => step.key === selected.key);
    const next =
      current === -1
        ? matches[delta === 1 ? 0 : matches.length - 1]
        : matches[(current + delta + matches.length) % matches.length];
    if (next) selectStep(next.key);
  };

  return (
    <section
      data-slot="inspect-page"
      className="@container flex min-h-[34rem] min-w-0 flex-1 flex-col overflow-hidden"
    >
      <InspectToolbar
        snapshot={snapshot}
        view={view}
        onViewChange={setView}
        fieldQuery={fieldQuery}
        onFieldQueryChange={setFieldQuery}
        onCycleMatch={cycleMatch}
        onRefresh={() => {
          void client.invalidateQueries({
            queryKey: queries.inspectRuntime().queryKey,
          });
          void client.invalidateQueries({
            queryKey: queries.inspectAppliedRuntime().queryKey,
          });
        }}
        loading={inspection.isFetching}
      />
      {inspection.isPending && (
        <p
          role="status"
          className="text-on-surface-variant flex flex-1 items-center justify-center p-6 text-sm"
        >
          {m.inspect_loading()}
        </p>
      )}
      {inspection.isError && (
        <p
          role="alert"
          className="text-error flex flex-1 items-center justify-center p-6 text-sm"
        >
          {m.inspect_error()} {String(inspection.error)}
        </p>
      )}
      {snapshot === null && (
        <p
          role="status"
          data-slot="inspect-empty"
          className="text-on-surface-variant flex flex-1 items-center justify-center p-6 text-sm"
        >
          {m.inspect_empty()}
        </p>
      )}
      {snapshot && selected && (
        <div
          className="flex min-h-0 flex-1 flex-col md:flex-row"
          data-slot="inspect-content"
        >
          <StepRail
            steps={visibleSteps}
            selectedKey={selected.key}
            onSelect={selectStep}
            profiles={profiles}
            matchedKeys={matchedKeys}
            onlyChanged={onlyChanged}
            onOnlyChangedChange={setOnlyChanged}
          />
          <StepDetail
            key={`${snapshot.snapshot_id}:${selected.key}`}
            snapshotId={snapshot.snapshot_id}
            step={selected}
            steps={steps}
            profiles={profiles}
            fieldQuery={fieldQuery}
          />
        </div>
      )}
    </section>
  );
}
