import type { ClashRule } from '@chimera/interface';
import type { RefObject } from 'react';
import { Virtualizer } from 'virtua';
import ContentDisplay from '@/components/base/content-display';
import * as m from '@/paraglide/messages';
import RuleItem from './rule-item';

interface RulePageProps {
  data: ClashRule[];
  scrollRef: RefObject<HTMLElement | null>;
  searchText?: string;
}

export const RulePage = ({ data, scrollRef, searchText }: RulePageProps) => {
  return data.length ? (
    <div className="h-full">
      <div className="sticky top-0 z-10 grid grid-cols-[5rem_10rem_minmax(0,1fr)_10rem] gap-4 border-b border-black/5 px-8 py-3 text-sm font-bold backdrop-blur-md dark:border-white/5">
        <div>{m.rules_column_index()}</div>
        <div>{m.rules_column_type()}</div>
        <div>{m.rules_column_payload()}</div>
        <div>{m.rules_column_proxy()}</div>
      </div>

      <Virtualizer scrollRef={scrollRef}>
        {data.map((item, index) => (
          <RuleItem
            key={`${item.type}-${item.payload}-${item.proxy}-${index}`}
            index={index}
            value={item}
            searchText={searchText}
          />
        ))}
      </Virtualizer>
    </div>
  ) : (
    <ContentDisplay className="absolute" message={m.rules_empty_message()} />
  );
};

export default RulePage;
