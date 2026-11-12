import {
  useClashCoreConfig,
  useClashInfo,
  useRuntimeProfile,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { Input } from '@chimera/ui/input';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@chimera/ui/modal';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useState } from 'react';
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

export default function ExternalControllerConfig() {
  return (
    <Modal>
      <ExternalControllerConfigForm />
    </Modal>
  );
}

function ExternalControllerConfigForm() {
  const { setOpen } = useModal();

  const { data, refetch } = useClashInfo();

  const { upsert } = useClashCoreConfig();

  const runtimeProfile = useRuntimeProfile();

  const [externalController, setExternalController] = useState(
    data?.server || '',
  );

  const handleSubmit = async () => {
    try {
      await upsert.mutateAsync({
        'external-controller': externalController,
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

  return (
    <SettingsCard data-slot="external-controller-config-card">
      <>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_external_controll_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>{data?.server}</ItemLabelDescription>
                </ItemLabel>

                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </ModalTrigger>
        </SettingsCardContent>

        <ModalContent
          dismissible={!upsert.isPending}
          onOpenAutoFocus={() => setExternalController(data?.server || '')}
        >
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
                value={externalController}
                onChange={(event) => setExternalController(event.target.value)}
              />
            </CardContent>

            <CardFooter className="gap-2">
              <Button
                variant="flat"
                onClick={() => void handleSubmit()}
                loading={upsert.isPending}
                disabled={upsert.isPending}
              >
                {m.common_apply()}
              </Button>

              <ModalClose disabled={upsert.isPending}>
                {m.common_close()}
              </ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </>
    </SettingsCard>
  );
}
