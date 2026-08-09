import { useClashCoreConfig, useClashInfo } from '@chimera/interface';
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
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
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
  const savedValue = data?.secret || '';
  const [draft, setDraft] = useState(savedValue);

  useEffect(() => setDraft(savedValue), [savedValue]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) setDraft(savedValue);
    setOpen(nextOpen);
  };

  const handleApply = async () => {
    try {
      await upsert.mutateAsync({ secret: draft });
      await refetch();
      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  };

  const handleCopy = async () => {
    if (!savedValue) return;

    try {
      await writeText(savedValue);
      message(m.settings_clash_settings_core_secret_copied(), {
        title: m.common_success(),
        kind: 'info',
      });
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  };

  return (
    <SettingsCard data-slot="core-secret-config-card">
      <Modal open={open} onOpenChange={handleOpenChange}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_core_secret_label()}
                  </ItemLabelText>
                  <ItemLabelDescription>{savedValue}</ItemLabelDescription>
                </ItemLabel>

                <div className="flex items-center gap-2">
                  <Button
                    variant="raised"
                    className="hover:bg-inverse-on-surface"
                    icon
                    aria-label={m.common_copy()}
                    onClick={(event) => {
                      event.stopPropagation();
                      void handleCopy();
                    }}
                    asChild
                  >
                    <span>
                      <ContentCopyRounded />
                    </span>
                  </Button>
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
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
              />
            </CardContent>
            <CardFooter className="gap-2">
              <Button
                variant="flat"
                loading={upsert.isPending}
                onClick={() => void handleApply()}
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
