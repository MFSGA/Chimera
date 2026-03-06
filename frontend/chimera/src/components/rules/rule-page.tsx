import { useAtomValue } from 'jotai';
import { useTranslation } from 'react-i18next';
import { Virtualizer } from 'virtua';
import ContentDisplay from '@/components/base/content-display';
import { atomRulePage } from './modules/store';
import RuleItem from './rule-item';

export const RulePage = () => {
  const { t } = useTranslation();
  const rule = useAtomValue(atomRulePage);

  return rule?.data?.length ? (
    <div className="h-full">
      <div className="sticky top-0 z-10 grid grid-cols-[5rem_10rem_minmax(0,1fr)_10rem] gap-4 border-b border-black/5 px-8 py-3 text-sm font-bold backdrop-blur-md dark:border-white/5">
        <div>Index</div>
        <div>Type</div>
        <div>Payload</div>
        <div>Proxy</div>
      </div>

      <Virtualizer scrollRef={rule.scrollRef}>
        {rule.data.map((item, index) => {
          return (
            <RuleItem
              key={index}
              index={index}
              value={item}
              searchText={rule.searchText}
            />
          );
        })}
      </Virtualizer>
    </div>
  ) : (
    <ContentDisplay className="absolute" message={t('No Rules')} />
  );
};

export default RulePage;
