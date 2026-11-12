import { useProfile } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { Input } from '@chimera/ui/input';
import {
  ModalClose,
  ModalContent,
  ModalTitle,
  useModal,
} from '@chimera/ui/modal';
import {
  SegmentedButton,
  SegmentedButtonItem,
} from '@chimera/ui/segmented-button';
import { useEffect, useState } from 'react';
import { useBlockTask } from '@/components/providers/block-task-provider';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { PROFILE_TYPE_NAMES, ProfileType } from './consts';
import { transformRequest } from './transform-profile-request';

const TRANSFORM_TYPES = [
  ProfileType.JavaScript,
  ProfileType.Lua,
  ProfileType.Merge,
] as const;

const defaultName = (type: ProfileType) =>
  PROFILE_TYPE_NAMES[type] + ' - ' + new Date().toLocaleString();

/** Local transform creation, shared by the library and transform-type pages. */
export default function CreateTransformModal({
  initialType = ProfileType.JavaScript,
}: {
  initialType?: ProfileType;
}) {
  const { open, setOpen } = useModal();
  const { create } = useProfile();
  const [type, setType] = useState<ProfileType>(initialType);
  const [name, setName] = useState(defaultName(initialType));
  const [desc, setDesc] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setType(initialType);
    setName(defaultName(initialType));
    setDesc('');
    setError(null);
  }, [open, initialType]);

  const task = useBlockTask('create-transform-profile-library', async () => {
    const nextName = name.trim();
    if (!nextName) {
      setError(m.profile_create_transform_name_required());
      return;
    }

    try {
      const { request, fileData } = transformRequest(
        type,
        nextName,
        desc.trim() || null,
      );
      await create.mutateAsync({
        type: 'manual',
        data: { request, fileData },
      });
      setOpen(false);
    } catch (cause) {
      await message(formatError(cause), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <ModalContent dismissible={!task.isPending}>
      <Card className="w-96 max-w-[calc(100vw-2rem)]">
        <CardHeader>
          <ModalTitle>{m.profile_create_transform_title()}</ModalTitle>
        </CardHeader>

        <CardContent className="space-y-4">
          <SegmentedButton
            size="sm"
            value={type}
            onValueChange={(value) => {
              if (value) {
                setType(value as ProfileType);
                setError(null);
              }
            }}
          >
            {TRANSFORM_TYPES.map((item) => (
              <SegmentedButtonItem key={item} value={item}>
                {PROFILE_TYPE_NAMES[item]}
              </SegmentedButtonItem>
            ))}
          </SegmentedButton>

          <div className="space-y-2">
            <Input
              variant="outlined"
              label={m.profile_form_name_label()}
              value={name}
              onChange={(event) => {
                setName(event.target.value);
                setError(null);
              }}
            />
            {error && (
              <p role="alert" className="text-error text-sm">
                {error}
              </p>
            )}
          </div>

          <Input
            variant="outlined"
            label={m.profile_form_desc_label()}
            value={desc}
            onChange={(event) => setDesc(event.target.value)}
          />
        </CardContent>

        <CardFooter className="gap-1">
          <Button
            onClick={() => void task.execute()}
            loading={task.isPending}
            disabled={task.isPending}
          >
            {m.common_submit()}
          </Button>

          <ModalClose disabled={task.isPending}>{m.common_cancel()}</ModalClose>
        </CardFooter>
      </Card>
    </ModalContent>
  );
}
