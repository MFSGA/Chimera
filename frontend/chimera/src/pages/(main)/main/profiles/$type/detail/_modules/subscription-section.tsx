import {
  getRemoteSource,
  useProfile,
  useProfileSyncStatus,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter } from '@chimera/ui/card';
import { LinearProgress } from '@chimera/ui/linear-progress';
import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import RuleSettingsRounded from '~icons/material-symbols/rule-settings-rounded';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import { useMemo } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { formatDate } from '@/utils/date';
import { message } from '@/utils/notification';
import { DetailRow, DetailSection } from './detail-section';
import { AutoUpdateSwitch, ProxySwitch } from './subscription-switches';
import SubscriptionUrlEditor from './subscription-url-editor';
import UpdateIntervalEditor from './update-interval-editor';
import UpdateOptionEditor from './update-option-editor';
import UserAgentEditor from './user-agent-editor';

const clampPercentage = (value: number) => Math.min(100, Math.max(0, value));

export default function SubscriptionSection({
  profile,
}: {
  profile: ProfileQueryResultItem;
}) {
  const { update } = useProfile();
  const syncStatus = useProfileSyncStatus(profile.uid);
  const remote = getRemoteSource(profile);
  const usage = useMemo(() => {
    if (!remote) return { progress: 0, total: 0, used: 0 };
    const total = remote.subscription?.total ?? 0;
    const used =
      (remote.subscription?.download ?? 0) + (remote.subscription?.upload ?? 0);
    return {
      total,
      used,
      progress: total > 0 ? clampPercentage((used / total) * 100) : 0,
    };
  }, [remote]);

  const task = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    async () => {
      try {
        await update.mutateAsync({ uid: profile.uid, option: null });
      } catch (error) {
        await message(`Update failed: \n ${formatError(error)}`, {
          title: m.common_error(),
          kind: 'error',
        });
      }
    },
  );
  const refresh = useLockFn(task.execute);

  if (!remote) return null;

  const updatedAt = remote.updated_at || null;
  const expire = remote.subscription?.expire || null;

  return (
    <DetailSection
      title={m.profile_detail_section_subscription()}
      className="col-span-2"
    >
      <SubscriptionUrlEditor profile={profile} asChild>
        <DetailRow
          label={m.profile_subscription_url_label()}
          value={remote.url}
        />
      </SubscriptionUrlEditor>
      <UserAgentEditor profile={profile} asChild>
        <DetailRow
          label={m.profile_user_agent_label()}
          value={
            remote.option.user_agent || m.profile_detail_user_agent_default()
          }
        />
      </UserAgentEditor>
      {remote.option.update_interval_minutes > 0 && (
        <UpdateIntervalEditor profile={profile} asChild>
          <DetailRow
            label={m.profile_update_interval_label()}
            value={m.profile_detail_auto_update_every({
              minutes: remote.option.update_interval_minutes,
            })}
          />
        </UpdateIntervalEditor>
      )}
      <Card>
        <CardContent>
          <div className="flex items-center justify-between text-sm font-bold">
            <span>{usage.progress.toFixed(2)}%</span>
            <span>
              {filesize(usage.used, { standard: 'iec' })} /{' '}
              {filesize(usage.total, { standard: 'iec' })}
            </span>
          </div>

          <LinearProgress value={usage.progress} />

          <div className="flex items-center justify-between gap-2 text-sm font-bold">
            <Tooltip>
              <TooltipTrigger asChild>
                <span>
                  {m.profile_subscription_updated_at({
                    updated: updatedAt
                      ? dayjs(updatedAt * 1000).fromNow()
                      : '-',
                  })}
                </span>
              </TooltipTrigger>
              {syncStatus.data?.next_run_at ? (
                <TooltipContent side="bottom">
                  {m.profile_subscription_next_update_at({
                    next: formatDate(syncStatus.data.next_run_at),
                  })}
                </TooltipContent>
              ) : null}
            </Tooltip>
            {expire ? (
              <span>
                {m.profile_subscription_expires_in({
                  expires: dayjs(expire * 1000).fromNow(),
                })}
              </span>
            ) : null}
          </div>
        </CardContent>

        <CardFooter className="gap-1">
          <Button
            className="flex items-center gap-2"
            data-slot="profile-manual-sync"
            data-profile-uid={profile.uid}
            onClick={() => void refresh()}
            loading={task.isPending}
          >
            <RefreshRounded className="size-4" />
            <span>{m.profile_subscription_update()}</span>
          </Button>

          <UpdateOptionEditor profile={profile} asChild>
            <Button
              className="flex items-center gap-2"
              data-slot="profile-update-options-trigger"
              data-profile-uid={profile.uid}
            >
              <RuleSettingsRounded className="size-4" />
              <span>{m.profile_update_option_edit()}</span>
            </Button>
          </UpdateOptionEditor>
        </CardFooter>
      </Card>
      <AutoUpdateSwitch
        profile={profile}
        intervalMinutes={remote.option.update_interval_minutes ?? 0}
      />
      <ProxySwitch
        profile={profile}
        channel="with_proxy"
        checked={remote.option.with_proxy ?? false}
      />
      <ProxySwitch
        profile={profile}
        channel="self_proxy"
        checked={remote.option.self_proxy ?? false}
      />
    </DetailSection>
  );
}
