import {
  commands,
  unwrapResult,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import type { ComponentProps } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileQueryResultItem;
}) {
  const task = useBlockTask(`open-profile-editor-${profile.uid}`, async () => {
    try {
      unwrapResult(await commands.createEditorWindow('profile', profile.uid));
    } catch (error) {
      await message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  const openEditor = useLockFn(task.execute);

  return (
    <Button
      {...props}
      loading={task.isPending}
      onClick={() => void openEditor()}
    />
  );
}
