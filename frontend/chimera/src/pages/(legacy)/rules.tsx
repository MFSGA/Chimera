import { useClashRules } from '@chimera/interface';
import { BasePage, cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import {
  lazy,
  Suspense,
  useDeferredValue,
  useMemo,
  useRef,
  useState,
} from 'react';
import * as m from '@/paraglide/messages';

const RulePageComponent = lazy(() => import('@/components/rules/rule-page'));

export const Route = createFileRoute('/(legacy)/rules')({
  component: RulesPage,
});

function RulesPage() {
  const { data } = useClashRules();
  const [filterText, setFilterText] = useState('');
  const deferredFilterText = useDeferredValue(filterText);
  const viewportRef = useRef<HTMLDivElement>(null);

  const filteredRules = useMemo(() => {
    const search = deferredFilterText.trim().toLowerCase();
    const rules = data?.rules ?? [];

    if (!search) {
      return rules;
    }

    return rules.filter((rule) =>
      [rule.type, rule.payload, rule.proxy].some((value) =>
        value?.toLowerCase().includes(search),
      ),
    );
  }, [data?.rules, deferredFilterText]);

  return (
    <BasePage
      full
      title={m.navbar_label_rules()}
      header={
        <input
          autoComplete="off"
          spellCheck="false"
          value={filterText}
          placeholder="Filter conditions"
          onChange={(event) => {
            setFilterText(event.target.value);
            viewportRef.current?.scrollTo({ top: 0 });
          }}
          className={cn(
            'bg-primary/10 h-10 min-w-0 rounded-full border-0 px-4 text-sm outline-none',
            'placeholder:text-on-surface-variant focus:ring-primary/40 focus:ring-2',
          )}
        />
      }
      viewportRef={viewportRef}
    >
      <Suspense fallback={null}>
        <RulePageComponent
          data={filteredRules}
          scrollRef={viewportRef}
          searchText={deferredFilterText}
        />
      </Suspense>
    </BasePage>
  );
}
