import {
  getRemoteSource,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { AnimatedItem } from '@chimera/ui/animated-item';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { Input, NumericInput } from '@chimera/ui/input';
import {
  Modal,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@chimera/ui/modal';
import { SwitchItem } from '@chimera/ui/switch';
import { zodResolver } from '@hookform/resolvers/zod';
import { AnimatePresence } from 'motion/react';
import { useCallback, useEffect, useState, type ComponentProps } from 'react';
import { Controller, useForm } from 'react-hook-form';
import { z } from 'zod';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import AnimatedErrorItem from '../../../_modules/error-item';

// Mirrors the backend default; saved when auto update is on and the interval
// is left empty. The subscription's suggested interval is applied at import.
const DEFAULT_UPDATE_INTERVAL_MINUTES = 120;

// The backend disables scheduled and startup syncs with zero minutes.
const AUTO_UPDATE_OFF_MINUTES = 0;

const formSchema = z.object({
  user_agent: z.string().optional(),
  with_proxy: z.boolean().optional(),
  self_proxy: z.boolean().optional(),
  auto_update: z.boolean(),
  update_interval: z
    .number()
    .min(1, { message: m.profile_form_option_update_interval_min_error() })
    .nullable()
    .optional(),
});

const remoteOptionOf = (profile: ProfileQueryResultItem) =>
  getRemoteSource(profile)?.option;

export default function UpdateOptionEditor({
  profile,
  ...props
}: ComponentProps<typeof ModalTrigger> & {
  profile: ProfileQueryResultItem;
}) {
  const { patchRemoteOptions } = useProfile();
  const [open, setOpen] = useState(false);

  const getDefaultValues = useCallback(() => {
    const option = remoteOptionOf(profile);
    const minutes = option?.update_interval_minutes;
    return {
      user_agent: option?.user_agent ?? '',
      with_proxy: option?.with_proxy ?? false,
      self_proxy: option?.self_proxy ?? false,
      auto_update: minutes !== AUTO_UPDATE_OFF_MINUTES,
      update_interval:
        minutes != null && minutes > AUTO_UPDATE_OFF_MINUTES ? minutes : null,
    };
  }, [profile]);

  const form = useForm<z.infer<typeof formSchema>>({
    resolver: zodResolver(formSchema),
    defaultValues: getDefaultValues(),
  });

  const autoUpdate = form.watch('auto_update');

  useEffect(() => {
    form.reset(getDefaultValues());
  }, [form, getDefaultValues]);

  const handleClose = () => {
    setOpen(false);
    form.reset(getDefaultValues());
  };

  const blockTask = useBlockTask(
    `update-remote-profile-${profile.uid}`,
    form.handleSubmit(
      async (data) => {
        try {
          await patchRemoteOptions.mutateAsync({
            uid: profile.uid,
            patch: {
              user_agent: data.user_agent || null,
              with_proxy: data.with_proxy ?? null,
              self_proxy: data.self_proxy ?? null,
              update_interval_minutes: data.auto_update
                ? (data.update_interval ?? DEFAULT_UPDATE_INTERVAL_MINUTES)
                : AUTO_UPDATE_OFF_MINUTES,
            },
          });
          handleClose();
        } catch (error) {
          message(`Update failed: \n ${formatError(error)}`, {
            title: m.common_error(),
            kind: 'error',
          });
        }
      },
      (error) => {
        console.error(error);
        message(formatError(error), {
          title: m.common_error(),
          kind: 'error',
        });
      },
    ),
  );

  const handleSubmit = useLockFn(blockTask.execute);
  const option = remoteOptionOf(profile);

  if (!option) return null;

  return (
    <Modal
      open={open}
      onOpenChange={(next) => {
        if (next) {
          form.reset(getDefaultValues());
          setOpen(true);
        } else {
          handleClose();
        }
      }}
    >
      <ModalTrigger {...props} />

      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>{m.profile_update_option_editor_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <Controller
              control={form.control}
              name="user_agent"
              render={({ field }) => (
                <div className="mt-2 flex items-center gap-2">
                  <Input
                    label={m.profile_user_agent_label()}
                    variant="outlined"
                    {...field}
                  />
                </div>
              )}
            />

            <Controller
              control={form.control}
              name="auto_update"
              render={({ field }) => (
                <SwitchItem
                  className="mt-2"
                  data-slot="profile-auto-update-switch"
                  checked={field.value}
                  onCheckedChange={field.onChange}
                >
                  <span>{m.profile_update_option_auto_update_label()}</span>
                </SwitchItem>
              )}
            />

            <AnimatePresence initial={false}>
              {autoUpdate && (
                <AnimatedItem key="update-interval">
                  <Controller
                    control={form.control}
                    name="update_interval"
                    render={({ field }) => (
                      <div className="mt-2 space-y-2">
                        <NumericInput
                          data-slot="profile-update-interval"
                          label={m.profile_update_interval_label()}
                          variant="outlined"
                          min={1}
                          step={1}
                          value={field.value}
                          onChange={field.onChange}
                        />
                        <p className="text-on-surface-variant px-1 text-xs">
                          {m.profile_update_option_interval_hint({
                            minutes: DEFAULT_UPDATE_INTERVAL_MINUTES,
                          })}
                        </p>
                        <AnimatePresence>
                          {form.formState.errors.update_interval && (
                            <AnimatedErrorItem className="text-error">
                              {form.formState.errors.update_interval.message}
                            </AnimatedErrorItem>
                          )}
                        </AnimatePresence>
                      </div>
                    )}
                  />
                </AnimatedItem>
              )}
            </AnimatePresence>

            <Controller
              control={form.control}
              name="with_proxy"
              render={({ field }) => (
                <SwitchItem
                  checked={field.value}
                  onCheckedChange={field.onChange}
                >
                  <span>{m.profile_with_proxy_label()}</span>
                </SwitchItem>
              )}
            />
            <Controller
              control={form.control}
              name="self_proxy"
              render={({ field }) => (
                <SwitchItem
                  checked={field.value}
                  onCheckedChange={field.onChange}
                >
                  <span>{m.profile_self_proxy_label()}</span>
                </SwitchItem>
              )}
            />
          </CardContent>

          <CardFooter className="gap-1">
            <Button
              data-slot="profile-update-options-save"
              onClick={() => void handleSubmit()}
              loading={blockTask.isPending}
            >
              {m.common_save()}
            </Button>
            <Button
              data-slot="profile-update-options-cancel"
              onClick={handleClose}
            >
              {m.common_cancel()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
}
