import {
  useClashConfig,
  useClashCoreConfig,
  useSetting,
} from '@chimera/interface';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useEffect, useMemo, useState } from 'react';
import { SwitchCard } from '@/components/settings/setting-control';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '@/components/settings/settings-card';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { NumericInput } from '@/components/ui/input';
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

const MIXED_PORT_FALLBACK = 7890;

export const MixedPortConfig = () => {
  const [open, setOpen] = useState(false);
  const mixedPort = useSetting('verge_mixed_port');
  const { query } = useClashConfig();
  const { upsert } = useClashCoreConfig();
  const current = useMemo(
    () => query.data?.['mixed-port'] || mixedPort.value || MIXED_PORT_FALLBACK,
    [mixedPort.value, query.data],
  );
  const [draft, setDraft] = useState<number | null>(current);

  useEffect(() => setDraft(current), [current]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) setDraft(current);
    setOpen(nextOpen);
  };

  const isValid =
    draft != null && Number.isInteger(draft) && draft >= 1 && draft <= 65535;

  const handleApply = async () => {
    if (!isValid || draft == null) return;

    try {
      await upsert.mutateAsync({ 'mixed-port': draft });
      await mixedPort.upsert(draft);
      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
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
                  <ItemLabelDescription>{current}</ItemLabelDescription>
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
                loading={upsert.isPending || mixedPort.isPending}
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
};

export const RandomPortSwitch = () => {
  const setting = useSetting('enable_random_port');

  const handleChange = async (checked: boolean) => {
    try {
      await setting.upsert(checked);
      message(m.settings_clash_port_restart_to_effect(), {
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
    <SwitchCard
      label={m.settings_clash_settings_random_port_label()}
      checked={Boolean(setting.value)}
      loading={setting.isPending}
      onCheckedChange={(checked) => void handleChange(checked)}
    />
  );
};
