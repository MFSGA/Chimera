import { queries, unwrapQueryOptions } from '@chimera/interface';
import { cn } from '@chimera/utils';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useRef, useState } from 'react';
import * as m from '@/paraglide/messages';
import ChangedFields from './changed-fields';
import { DiffViewer, YamlViewer } from './inspect-document';
import { stageLabel, stepHeadline } from './inspect-labels';
import { FINAL_KEY, type Step } from './inspect-steps';
import { fieldLocator, type FieldJump } from './text-find';

type InspectTab = 'changes' | 'output' | 'logs';

export default function StepDetail({
  snapshotId,
  step,
  steps,
  profiles,
  fieldQuery,
}: {
  snapshotId: string;
  step: Step;
  steps: Step[];
  profiles: ReadonlyMap<string, string>;
  fieldQuery: string;
}) {
  const [tab, setTab] = useState<InspectTab>(
    step.key === FINAL_KEY ? 'output' : 'changes',
  );
  const [jump, setJump] = useState<FieldJump>();
  const jumpNonce = useRef(0);
  const query = queries.inspectRuntimeNode(snapshotId, step.node.id);
  const content = useQuery({
    ...unwrapQueryOptions(query, query.queryFn!),
    retry: false,
    staleTime: Infinity,
    refetchOnWindowFocus: false,
  });

  const data = content.data;
  const locate = useMemo(
    () => fieldLocator(data?.yaml.split('\n') ?? []),
    [data?.yaml],
  );
  const position = steps.findIndex((item) => item.key === step.key) + 1;

  const selectField = (field: string) => {
    const place = locate(field);
    if (!place) return;
    setJump({ field, ...place, nonce: ++jumpNonce.current });
    setTab('output');
  };

  const tabs: { id: InspectTab; label: string }[] = [
    { id: 'changes', label: m.inspect_tab_changes() },
    { id: 'output', label: m.inspect_tab_output() },
    {
      id: 'logs',
      label: data?.logs.length
        ? `${m.inspect_tab_logs()} (${data.logs.length})`
        : m.inspect_tab_logs(),
    },
  ];

  return (
    <section
      data-slot="inspect-step-detail"
      data-key={step.key}
      className="flex min-h-[20rem] min-w-0 flex-1 flex-col overflow-hidden"
    >
      <header className="border-outline-variant flex shrink-0 flex-col gap-2.5 border-b px-5 pt-4 pb-3">
        <p className="text-primary text-xs font-semibold">
          {stageLabel(step.stage)} · {position}/{steps.length}
        </p>
        <h2 className="text-on-surface truncate text-xl font-semibold">
          {stepHeadline(step, profiles)}
        </h2>
        <div className="flex flex-wrap items-center gap-3">
          <div
            role="tablist"
            aria-label={m.inspect_steps()}
            className="bg-surface-variant flex w-fit rounded-full p-1"
          >
            {tabs.map((item) => (
              <button
                key={item.id}
                type="button"
                role="tab"
                aria-selected={tab === item.id}
                data-slot={`inspect-tab-${item.id}`}
                className={cn(
                  'h-8 rounded-full px-3 text-sm',
                  'focus-visible:outline-primary focus-visible:outline-2',
                  tab === item.id
                    ? 'bg-secondary-container text-on-secondary-container font-semibold'
                    : 'text-on-surface-variant hover:bg-surface',
                )}
                onClick={() => setTab(item.id)}
              >
                {item.label}
              </button>
            ))}
          </div>
          {(step.changedCount ?? 0) > 0 && (
            <span
              data-slot="inspect-step-summary"
              className="text-on-surface-variant text-xs"
            >
              {step.changedCount} {m.inspect_tab_changes()}
            </span>
          )}
        </div>
        {tab === 'changes' && (
          <ChangedFields
            fields={step.node.changed_fields}
            query={fieldQuery}
            canSelect={(field) => Boolean(data && locate(field))}
            onSelect={selectField}
          />
        )}
      </header>

      <div
        role="tabpanel"
        data-slot="inspect-step-panel"
        data-tab={tab}
        className="bg-code flex min-h-0 flex-1 flex-col"
      >
        {content.isPending && (
          <p role="status" className="text-on-surface-variant p-6 text-sm">
            {m.inspect_loading()}
          </p>
        )}
        {content.isError && (
          <p role="alert" className="text-error p-6 text-sm">
            {m.inspect_content_error()} {String(content.error)}
          </p>
        )}
        {data && tab === 'output' && (
          <YamlViewer code={data.yaml} jump={jump} label={m.inspect_yaml()} />
        )}
        {data &&
          tab === 'changes' &&
          (data.diff && data.diff.hunks.length > 0 ? (
            <>
              <p className="text-on-surface-variant shrink-0 px-5 py-2 text-xs">
                {m.inspect_diff()}
                {(() => {
                  const parent = steps.find(
                    (item) => item.node.id === data.diff?.parent_id,
                  );
                  return parent ? ` · ${stepHeadline(parent, profiles)}` : '';
                })()}
              </p>
              <DiffViewer hunks={data.diff.hunks} label={m.inspect_diff()} />
            </>
          ) : (
            <p role="status" className="text-on-surface-variant p-6 text-sm">
              {data.diff ? m.inspect_unchanged() : m.inspect_independent()}
            </p>
          ))}
        {data &&
          tab === 'logs' &&
          (data.logs.length === 0 ? (
            <p role="status" className="text-on-surface-variant p-6 text-sm">
              {m.inspect_no_logs()}
            </p>
          ) : (
            <ol
              data-slot="inspect-step-logs"
              className="flex min-h-0 flex-1 flex-col gap-1 overflow-auto p-3"
            >
              {data.logs.map((log, index) => (
                <li
                  key={index}
                  className="bg-surface flex items-start gap-2.5 rounded-xl px-3 py-2.5 text-sm"
                >
                  <span className="text-on-surface-variant font-mono text-xs font-semibold uppercase">
                    {log.level}
                  </span>
                  <span className="min-w-0 flex-1 break-words whitespace-pre-wrap">
                    {log.message}
                  </span>
                </li>
              ))}
            </ol>
          ))}
      </div>
    </section>
  );
}
