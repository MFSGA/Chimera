import { cn } from '@chimera/utils';
import * as m from '@/paraglide/messages';
import { stageLabel, stepHeadline } from './inspect-labels';
import { STAGES, type Step } from './inspect-steps';

export default function StepRail({
  steps,
  selectedKey,
  onSelect,
  profiles,
  matchedKeys,
  onlyChanged,
  onOnlyChangedChange,
}: {
  steps: Step[];
  selectedKey?: string;
  onSelect: (key: string) => void;
  profiles: ReadonlyMap<string, string>;
  matchedKeys?: ReadonlySet<string>;
  onlyChanged: boolean;
  onOnlyChangedChange: (value: boolean) => void;
}) {
  return (
    <aside
      data-slot="inspect-step-rail"
      aria-label={m.inspect_steps()}
      className="bg-pane border-outline-variant flex min-h-0 w-full shrink-0 flex-col border-b md:w-72 md:border-r md:border-b-0"
    >
      <div className="flex items-center justify-between gap-2 px-4 py-3">
        <h2 className="text-on-surface text-sm font-semibold">
          {m.inspect_steps()}
        </h2>
        <label className="text-on-surface-variant flex cursor-pointer items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={onlyChanged}
            onChange={(event) => onOnlyChangedChange(event.target.checked)}
            data-slot="inspect-only-changed"
            className="accent-primary size-4"
          />
          {m.inspect_only_changed()}
        </label>
      </div>
      <div
        data-slot="inspect-step-list"
        className="max-h-56 min-h-0 flex-1 overflow-auto px-3 pb-4 md:max-h-none"
      >
        {steps.length === 0 && (
          <p role="status" className="text-on-surface-variant p-3 text-sm">
            {m.inspect_no_matches()}
          </p>
        )}
        {STAGES.filter((stage) =>
          steps.some((step) => step.stage === stage),
        ).map((stage) => (
          <section key={stage} data-stage={stage} className="mb-4">
            <h3 className="text-primary px-2 py-2 text-xs font-semibold">
              {stageLabel(stage)}
            </h3>
            <div className="flex flex-col gap-1">
              {steps
                .filter((step) => step.stage === stage)
                .map((step) => {
                  const selected = step.key === selectedKey;
                  const matched = matchedKeys?.has(step.key);
                  return (
                    <button
                      key={step.key}
                      type="button"
                      data-slot="inspect-step-select"
                      data-key={step.key}
                      data-node-id={step.node.id}
                      data-status={step.status}
                      aria-current={selected ? 'step' : undefined}
                      className={cn(
                        'text-on-surface flex min-h-12 w-full items-center gap-2 rounded-2xl px-3 py-2 text-left text-sm',
                        'hover:bg-surface-variant/70 focus-visible:outline-primary focus-visible:outline-2',
                        selected
                          ? 'bg-secondary-container text-on-secondary-container'
                          : 'bg-surface',
                        step.depth === 1 && 'ml-4 w-[calc(100%-1rem)]',
                        matchedKeys && !matched && 'opacity-55',
                      )}
                      onClick={() => onSelect(step.key)}
                    >
                      <span
                        aria-hidden
                        className={cn(
                          'size-2 shrink-0 rounded-full',
                          step.status === 'changed' && 'bg-primary',
                          step.status === 'final' && 'bg-ok',
                          step.status === 'core' && 'bg-ok',
                          step.status === 'warn' && 'bg-warn',
                          step.status === 'error' && 'bg-error',
                          step.status === 'start' && 'bg-on-surface-variant',
                          step.status === 'unchanged' &&
                            'border-on-surface-variant border',
                        )}
                      />
                      <span className="min-w-0 flex-1 truncate">
                        {stepHeadline(step, profiles)}
                      </span>
                      {step.node.has_logs && (
                        <span
                          className="bg-surface-variant text-on-surface-variant rounded-md px-1.5 py-0.5 text-[10px]"
                          title={m.inspect_has_logs()}
                        >
                          LOG
                        </span>
                      )}
                      {(step.changedCount ?? 0) > 0 && (
                        <span className="text-on-surface-variant text-xs tabular-nums">
                          {step.changedCount}
                        </span>
                      )}
                    </button>
                  );
                })}
            </div>
          </section>
        ))}
      </div>
    </aside>
  );
}
