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

export default function ProfileNameEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileQueryResultItem;
}) {
  const { patchMetadata } = useProfile();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(profile.name);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setOpen(false);
    setName(profile.name);
    setError(null);
  };

  const task = useBlockTask(`update-profile-name-${profile.uid}`, async () => {
    const nextName = name.trim();
    if (!nextName) {
      setError(m.profile_name_label());
      return;
    }

    try {
      await patchMetadata.mutateAsync({
        uid: profile.uid,
        patch: { name: nextName },
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
            <ModalTitle>{m.profile_name_editor_title()}</ModalTitle>
          </CardHeader>

          <CardContent className="gap-2">
            <Input
              label={m.profile_name_label()}
              variant="outlined"
              value={name}
              onChange={(event) => {
                setName(event.target.value);
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
