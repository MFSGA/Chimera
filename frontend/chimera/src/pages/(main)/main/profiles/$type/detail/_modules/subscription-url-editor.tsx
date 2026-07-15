import {
  remoteProfileDefinitionOf,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { useState, type ComponentProps } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export default function SubscriptionUrlEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileQueryResultItem;
}) {
  const { replaceDefinition, update } = useProfile();
  const [open, setOpen] = useState(false);
  const [url, setUrl] = useState(profile.type === 'remote' ? profile.url : '');
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setOpen(false);
    setUrl(profile.type === 'remote' ? profile.url : '');
    setError(null);
  };

  const task = useBlockTask(
    `update-remote-profile-url-${profile.uid}`,
    async () => {
      const definition = remoteProfileDefinitionOf(profile);
      if (!definition) return;

      let parsed: URL;
      try {
        parsed = new URL(url);
        if (!['http:', 'https:'].includes(parsed.protocol)) throw new Error();
      } catch {
        setError(m.profile_subscription_url_label());
        return;
      }

      try {
        await replaceDefinition.mutateAsync({
          uid: profile.uid,
          definition: {
            ...definition,
            config: {
              ...definition.config,
              source: {
                ...definition.config.source,
                url: parsed.toString(),
              },
            },
          },
        });
        await update.mutateAsync({ uid: profile.uid, option: null });
        close();
      } catch (cause) {
        await message(`Update failed: \n ${formatError(cause)}`, {
          title: m.common_error(),
          kind: 'error',
        });
      }
    },
  );
  const submit = useLockFn(task.execute);

  if (profile.type !== 'remote') return null;

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
            <ModalTitle>{m.profile_subscription_url_editor_label()}</ModalTitle>
          </CardHeader>

          <CardContent className="gap-2">
            <Input
              label={m.profile_subscription_url_label()}
              variant="outlined"
              value={url}
              onChange={(event) => {
                setUrl(event.target.value);
                setError(null);
              }}
            />
            {error && <p className="text-error text-sm">{error}</p>}
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
