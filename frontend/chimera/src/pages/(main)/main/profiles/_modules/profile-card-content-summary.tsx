import { useProfileContent } from '@chimera/interface';
import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import AccountTreeOutlineRounded from '~icons/material-symbols/account-tree-outline-rounded';
import LayersOutlineRounded from '~icons/material-symbols/layers-outline-rounded';
import RuleRounded from '~icons/material-symbols/rule-rounded';
import * as m from '@/paraglide/messages';
import { useProfileContentSummary } from './use-profile-content-summary';

export default function ProfileCardContentSummary({ uid }: { uid: string }) {
  const { query } = useProfileContent(uid);
  const summary = useProfileContentSummary(uid, query.data);
  const items = [
    {
      label: m.profile_card_nodes(),
      value: summary?.nodes,
      Icon: AccountTreeOutlineRounded,
    },
    {
      label: m.profile_card_groups(),
      value: summary?.groups,
      Icon: LayersOutlineRounded,
    },
    { label: m.profile_card_rules(), value: summary?.rules, Icon: RuleRounded },
  ];

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          data-slot="profile-card-content-summary"
          className="text-on-surface-variant flex flex-wrap gap-x-4 gap-y-1 text-xs"
        >
          {items.map(({ label, value, Icon }) => (
            <span key={label} className="inline-flex items-center gap-1">
              <Icon className="size-3.5" aria-hidden />
              {label}
              <span className="text-on-surface font-medium tabular-nums">
                {value ?? '—'}
              </span>
            </span>
          ))}
        </div>
      </TooltipTrigger>
      <TooltipContent>
        {summary
          ? m.profile_card_content_summary_hint()
          : m.profile_card_content_summary_unavailable()}
      </TooltipContent>
    </Tooltip>
  );
}
