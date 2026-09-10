import type { ClashRule } from '@chimera/interface';
import { cn } from '@chimera/utils';
import HighlightText from '@/components/ui/highlight-text';

interface Props {
  index: number;
  value: ClashRule;
  searchText?: string;
}

const PROXY_COLORS = [
  'text-primary',
  'text-secondary',
  'text-tertiary',
  'text-warning',
  'text-success',
] as const;

const getProxyColor = (text: string) => {
  if (text === 'REJECT' || text === 'REJECT-DROP') {
    return 'text-error';
  }

  if (text === 'DIRECT') {
    return 'text-on-surface';
  }

  let sum = 0;
  for (let index = 0; index < text.length; index += 1) {
    sum += text.charCodeAt(index);
  }

  return PROXY_COLORS[sum % PROXY_COLORS.length];
};

const RuleItem = ({ index, value, searchText = '' }: Props) => {
  return (
    <div className="grid grid-cols-[5rem_10rem_minmax(0,1fr)_10rem] items-start gap-4 border-b border-black/5 px-8 py-3 select-text last:border-b-0 dark:border-white/5">
      <div className="text-on-surface-variant text-sm tabular-nums">
        {index + 1}
      </div>

      <HighlightText
        className="min-w-0 text-sm font-medium"
        searchText={searchText}
      >
        {value.type || '-'}
      </HighlightText>

      <HighlightText className="min-w-0 break-all" searchText={searchText}>
        {value.payload || '-'}
      </HighlightText>

      <HighlightText
        className={cn(
          'min-w-0 text-sm font-medium break-all',
          getProxyColor(value.proxy),
        )}
        searchText={searchText}
      >
        {value.proxy}
      </HighlightText>
    </div>
  );
};

export default RuleItem;
