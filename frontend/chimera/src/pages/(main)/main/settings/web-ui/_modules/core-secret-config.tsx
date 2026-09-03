import {
  useClashCoreConfig,
  useClashInfo,
  useRuntimeProfile,
} from '@chimera/interface';
import { writeText } from '@tauri-apps/plugin-clipboard-manager';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import ContentCopyRounded from '~icons/material-symbols/content-copy-rounded';
import { useEffect, useState } from 'react';
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
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError, sleep } from '@/utils';
import { message } from '@/utils/notification';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function CoreSecretConfig() {
  const [open, setOpen] = useState(false);

  const { data, refetch } = useClashInfo();

  const { upsert } = useClashCoreConfig();

  const runtimeProfile = useRuntimeProfile();

  const [coreSecret, setCoreSecret] = useState(data?.secret || '');

  useEffect(() => {
    setCoreSecret(data?.secret || '');
  }, [data?.secret]);

  const handleSubmit = async () => {
    try {
      await upsert.mutateAsync({
        secret: coreSecret,
      });
      await refetch();

      await sleep(300);
      await runtimeProfile.refetch();

      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      });
    }
  };

  const handleCopyClick = useLockFn(async () => {
    if (!data?.secret) {
      return;
    }

    try {
      await writeText(data.secret);

      message(m.settings_clash_settings_core_secret_copied(), {
        title: 'Success',
        kind: 'info',
      });
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  return (
    <SettingsCard data-slot="core-secret-config-card">
      <Modal open={open} onOpenChange={setOpen}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_core_secret_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>{data?.secret}</ItemLabelDescription>
                </ItemLabel>

                <div className="flex items-center gap-2">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="raised"
                        className="hover:bg-inverse-on-surface"
                        icon
                        aria-label={m.common_copy()}
                        onClick={(event) => {
                          event.stopPropagation();
                          void handleCopyClick();
                        }}
                        asChild
                      >
                        <span>
                          <ContentCopyRounded />
                        </span>
                      </Button>
                    </TooltipTrigger>

                    <TooltipContent>{m.common_copy()}</TooltipContent>
                  </Tooltip>

                  <ArrowForwardIosRounded />
                </div>
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <ModalContent>
          <Card className="flex min-w-96 flex-col">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_settings_core_secret_label_edit()}
              </ModalTitle>
            </CardHeader>

            <CardContent>
              <Input
                variant="outlined"
                label={m.settings_clash_settings_core_secret_label()}
                value={coreSecret}
                onChange={(event) => setCoreSecret(event.target.value)}
              />
            </CardContent>

            <CardFooter className="gap-2">
              <Button
                variant="flat"
                onClick={() => void handleSubmit()}
                loading={upsert.isPending}
              >
                {m.common_apply()}
              </Button>

              <ModalClose>{m.common_close()}</ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </SettingsCard>
  );
}
