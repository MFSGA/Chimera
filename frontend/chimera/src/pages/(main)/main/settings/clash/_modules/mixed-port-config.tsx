import { useClashConfig, useClashSetting } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { NumericInput } from '@chimera/ui/input';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@chimera/ui/modal';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useEffect, useMemo, useState } from 'react';
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

const DEFAULT_MIXED_PORT = 7890;

export default function MixedPortConfig() {
  const [open, setOpen] = useState(false);

  const mixedPort = useClashSetting('mixed_port');

  const clashConfig = useClashConfig();

  const currentMixedPort = useMemo(
    () =>
      clashConfig.query.data?.['mixed-port'] ||
      mixedPort.value?.start_port ||
      DEFAULT_MIXED_PORT,
    [clashConfig.query.data, mixedPort.value],
  );

  const [draft, setDraft] = useState<number | null>(currentMixedPort);

  useEffect(() => {
    setDraft(currentMixedPort);
  }, [currentMixedPort]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) {
      setDraft(currentMixedPort);
    }
    setOpen(nextOpen);
  };

  const isValid =
    draft != null && Number.isInteger(draft) && draft >= 1 && draft <= 65535;

  const handleSubmit = async () => {
    if (!isValid || draft == null) {
      return;
    }

    try {
      await mixedPort.upsert({ start_port: draft });

      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      });
    }
  };

  return (
    <SettingsCard data-slot="mixed-port-config-card">
      <Modal open={open} onOpenChange={handleOpenChange}>
        <SettingsCardContent asChild>
          <ModalTrigger asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_clash_settings_mixed_port_label()}
                  </ItemLabelText>

                  <ItemLabelDescription>
                    {currentMixedPort}
                  </ItemLabelDescription>
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
                {m.settings_clash_settings_mixed_port_label_edit()}
              </ModalTitle>
            </CardHeader>

            <CardContent>
              <NumericInput
                variant="outlined"
                label={m.settings_clash_settings_mixed_port_label()}
                value={draft}
                min={1}
                max={65535}
                allowNegative={false}
                decimalScale={0}
                onChange={setDraft}
              />
            </CardContent>

            <CardFooter className="gap-2">
              <Button
                variant="flat"
                disabled={!isValid}
                loading={mixedPort.isPending}
                onClick={() => void handleSubmit()}
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
