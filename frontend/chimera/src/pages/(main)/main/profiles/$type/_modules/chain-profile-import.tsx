import {
  ProfileTemplate,
  useProfile,
  type ProfileBuilderRequest_Deserialize,
} from '@chimera/interface';
import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded';
import { useMemo, useState } from 'react';
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
import { PROFILE_TYPE_NAMES, ProfileType } from '../../_modules/consts';
import AnimatedErrorItem from '../../_modules/error-item';
import { Route as IndexRoute } from '../index';

const defaultName = (type: ProfileType) =>
  `${PROFILE_TYPE_NAMES[type]} - ${new Date().toLocaleString()}`;

const transformRequest = (
  type: ProfileType,
  name: string,
  desc: string | null,
): { item: ProfileBuilderRequest_Deserialize; fileData: string } => {
  if (type === ProfileType.Merge) {
    return {
      item: { type: 'merge', name, desc },
      fileData: ProfileTemplate.merge,
    };
  }

  const scriptType =
    type === ProfileType.Lua ? ('lua' as const) : ('javascript' as const);
  return {
    item: { type: 'script', name, desc, script_type: scriptType },
    fileData:
      scriptType === 'lua'
        ? ProfileTemplate.luascript
        : ProfileTemplate.javascript,
  };
};

export default function ChainProfileImport() {
  const { type: rawType } = IndexRoute.useParams();
  const type = rawType as ProfileType;
  const { create } = useProfile();
  const [open, setOpen] = useState(false);
  const initialName = useMemo(() => defaultName(type), [type]);
  const [name, setName] = useState(initialName);
  const [desc, setDesc] = useState('');
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    setOpen(false);
    setName(defaultName(type));
    setDesc('');
    setError(null);
  };

  const task = useBlockTask(`create-transform-profile-${type}`, async () => {
    const nextName = name.trim();
    if (!nextName) {
      setError(m.profile_form_name_label());
      return;
    }

    try {
      const { item, fileData } = transformRequest(
        type,
        nextName,
        desc.trim() || null,
      );
      await create.mutateAsync({
        type: 'manual',
        data: { item, fileData },
      });
      close();
    } catch (cause) {
      await message(formatError(cause), {
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
        if (task.isPending) return;
        if (next) {
          setName(defaultName(type));
          setDesc('');
          setError(null);
          setOpen(true);
        } else {
          close();
        }
      }}
    >
      <ModalTrigger asChild>
        <Button
          variant="fab"
          icon
          aria-label={m.profile_import_chain_title({
            type: PROFILE_TYPE_NAMES[type],
          })}
        >
          <NoteStackAddRounded className="size-6" />
        </Button>
      </ModalTrigger>

      <ModalContent>
        <Card className="w-96 max-w-[calc(100vw-2rem)]">
          <CardHeader>
            <ModalTitle>
              {m.profile_import_chain_title({ type: PROFILE_TYPE_NAMES[type] })}
            </ModalTitle>
          </CardHeader>

          <CardContent className="space-y-4">
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
                <AnimatedErrorItem className="text-error">
                  {error}
                </AnimatedErrorItem>
              )}
            </div>

            <Input
              variant="outlined"
              label={m.profile_form_desc_label()}
              value={desc}
              onChange={(event) => setDesc(event.target.value)}
            />
          </CardContent>

          <CardFooter className="gap-2">
            <Button onClick={submit} loading={task.isPending}>
              {m.common_submit()}
            </Button>
            <Button onClick={close} disabled={task.isPending}>
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
}
