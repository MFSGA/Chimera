import {
  useClashCoreConfig,
  useClashInfo,
  useRuntimeProfile,
} from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
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

export default function ExternalControllerConfig() {
  const [open, setOpen] = useState(false);
  const { data, refetch } = useClashInfo();
  const { upsert } = useClashCoreConfig();
  const runtimeProfile = useRuntimeProfile();
  const savedValue = data?.server || '';
  const [draft, setDraft] = useState(savedValue);

  useEffect(() => setDraft(savedValue), [savedValue]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) setDraft(savedValue);
    setOpen(nextOpen);
  };

  const handleApply = async () => {
    try {
      await upsert.mutateAsync({ 'external-controller': draft });
      await refetch();
      await new Promise((resolve) => setTimeout(resolve, 300));
      await runtimeProfile.refetch();
      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  };

  return (
    <SettingsCard data-slot="external-controller-config-card">
      <Modal open={open} onOpenChange={handleOpenChange}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_external_controll_label()}
                  </ItemLabelText>
                  <ItemLabelDescription>{savedValue}</ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <ModalContent>
          <Card className="flex min-w-96 flex-col">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_settings_external_controll_label_edit()}
              </ModalTitle>
            </CardHeader>
            <CardContent>
              <Input
                variant="outlined"
                label={m.settings_clash_settings_external_controll_label()}
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
