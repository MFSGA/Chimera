import {
  commands,
  unwrapResult,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import type { ComponentProps } from 'react';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';

export default function ViewContent({
  profile,
  ...props
}: Omit<ComponentProps<typeof Button>, 'loading' | 'onClick'> & {
  profile: ProfileQueryResultItem;
}) {
  const openEditor = useLockFn(async () => {
    unwrapResult(await commands.createEditorWindow('profile', profile.uid));
  });

  return <Button {...props} onClick={() => void openEditor()} />;
}
