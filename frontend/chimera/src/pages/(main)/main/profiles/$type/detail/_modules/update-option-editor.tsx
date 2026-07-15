import { useProfile, type ProfileQueryResultItem } from '@chimera/interface';
import { useState, type ComponentProps } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { Input, NumericInput } from '@/components/ui/input';
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import { SwitchItem } from '@/components/ui/switch';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export default function UpdateOptionEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileQueryResultItem;
}) {
  const remote = profile.type === 'remote' ? profile : null;
  const { patchRemoteOptions } = useProfile();
  const [open, setOpen] = useState(false);
  const [userAgent, setUserAgent] = useState(remote?.option.user_agent ?? '');
  const [withProxy, setWithProxy] = useState(
    remote?.option.with_proxy ?? false,
  );
  const [selfProxy, setSelfProxy] = useState(
    remote?.option.self_proxy ?? false,
  );
  const [interval, setInterval] = useState<number | null>(
    remote?.option.update_interval_minutes ?? null,
  );
  const [error, setError] = useState<string | null>(null);

  const reset = () => {
    setUserAgent(remote?.option.user_agent ?? '');
    setWithProxy(remote?.option.with_proxy ?? false);
    setSelfProxy(remote?.option.self_proxy ?? false);
    setInterval(remote?.option.update_interval_minutes ?? null);
    setError(null);
  };
  const close = () => {
    setOpen(false);
    reset();
  };

  const task = useBlockTask(`patch-remote-options-${profile.uid}`, async () => {
    if (!remote) return;
    if (interval != null && interval < 1) {
      setError(m.profile_update_interval_label());
      return;
    }

    try {
      await patchRemoteOptions.mutateAsync({
        uid: remote.uid,
        patch: {
          user_agent: userAgent.trim() || null,
          with_proxy: withProxy,
          self_proxy: selfProxy,
          update_interval_minutes: interval,
        },
      });
      close();
    } catch (cause) {
      await message(`Update failed: \n ${formatError(cause)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  const submit = useLockFn(task.execute);

  if (!remote) return null;

  return (
    <Modal
      open={open}
      onOpenChange={(next) => {
        if (next) setOpen(true);
        else close();
      }}
    >
      <ModalTrigger {...props} />
      <ModalContent>
        <Card className="w-96 max-w-[calc(100vw-2rem)]">
          <CardHeader>
            <ModalTitle>{m.profile_update_option_editor_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <Input
              label={m.profile_user_agent_label()}
              variant="outlined"
              value={userAgent}
              onChange={(event) => setUserAgent(event.target.value)}
            />
            <NumericInput
              label={m.profile_update_interval_label()}
              variant="outlined"
              min={1}
              value={interval}
              onChange={(value) => {
                setInterval(value);
                setError(null);
              }}
            />
            {error && <p className="text-error text-sm">{error}</p>}
            <SwitchItem checked={withProxy} onCheckedChange={setWithProxy}>
              <span>{m.profile_with_proxy_label()}</span>
            </SwitchItem>
            <SwitchItem checked={selfProxy} onCheckedChange={setSelfProxy}>
              <span>{m.profile_self_proxy_label()}</span>
            </SwitchItem>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={() => void submit()} loading={task.isPending}>
              {m.common_save()}
            </Button>
            <Button onClick={close}>{m.common_cancel()}</Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
}
