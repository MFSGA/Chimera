import { useClashSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function RandomPortSwitch() {
  const enableRandomPort = useClashSetting('mixed_port');

  const handleRandomPort = async () => {
    try {
      await enableRandomPort.upsert({
        kind: enableRandomPort.value?.kind === 'random' ? 'fixed' : 'random',
      });
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      });
      return;
    }

    message(
      enableRandomPort.value?.kind === 'random'
        ? m.settings_clash_settings_random_port_disabled()
        : m.settings_clash_settings_random_port_enabled(),
      {
        title: 'Successful',
        kind: 'info',
      },
    );
  };

  return (
    <ItemContainer data-slot="random-port-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_random_port_label()}
        </ItemLabelText>
      </ItemLabel>

      <Switch
        checked={enableRandomPort.value?.kind === 'random'}
        onCheckedChange={() => void handleRandomPort()}
        loading={enableRandomPort.isPending}
      />
    </ItemContainer>
  );
}
