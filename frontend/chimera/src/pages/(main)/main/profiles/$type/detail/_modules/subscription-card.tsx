import {
  getNextProfileUpdateTimestamp,
  getProfileSubscriptionUsage,
  getSafeProfileTimestamp,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import RuleSettingsRounded from '~icons/material-symbols/rule-settings-rounded';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import { useMemo } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { LinearProgress } from '@/components/ui/linear-progress';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import UpdateOptionEditor from './update-option-editor';

export default function SubscriptionCard({
  profile,
}: {
  profile: ProfileQueryResultItem;
}) {
  const { update } = useProfile();
  const usage = useMemo(() => {
    if (profile.type !== 'remote') return { progress: 0, total: 0, used: 0 };
    return getProfileSubscriptionUsage(
      profile.extra?.upload,
      profile.extra?.download,
      profile.extra?.total,
    );
  }, [profile]);

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

  if (profile.type !== 'remote') return null;

  const updatedAt = getSafeProfileTimestamp(profile.updated);
  const expire = getSafeProfileTimestamp(profile.extra?.expire);
  const nextUpdate = getNextProfileUpdateTimestamp(
    updatedAt,
    profile.option.update_interval_minutes,
  );

  return (
    <Card className="col-span-2">
      <CardHeader>{m.profile_subscription_title()}</CardHeader>

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
          <span
            title={
              nextUpdate
                ? dayjs(nextUpdate * 1000).format('YYYY-MM-DD HH:mm:ss')
                : undefined
            }
          >
            {m.profile_subscription_updated_at({
              updated: updatedAt ? dayjs(updatedAt * 1000).fromNow() : '-',
            })}
          </span>
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
          onClick={() => void refresh()}
          loading={task.isPending}
        >
          <RefreshRounded className="size-4" />
          <span>{m.profile_subscription_update()}</span>
        </Button>

        <UpdateOptionEditor profile={profile} asChild>
          <Button className="flex items-center gap-2">
            <RuleSettingsRounded className="size-4" />
            <span>{m.profile_update_option_edit()}</span>
          </Button>
        </UpdateOptionEditor>
      </CardFooter>
    </Card>
  );
}
