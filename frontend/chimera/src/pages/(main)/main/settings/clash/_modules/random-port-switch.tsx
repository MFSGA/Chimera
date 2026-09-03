import { useSetting } from '@chimera/interface';
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
  const enableRandomPort = useSetting('enable_random_port');

  const handleRandomPort = async () => {
    try {
      await enableRandomPort.upsert(!enableRandomPort.value);
    } catch (error) {
      message(formatError(error), {
        title: 'Error',
        kind: 'error',
      });
      return;
    }

    message(
      enableRandomPort.value
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
        checked={Boolean(enableRandomPort.value)}
        onCheckedChange={() => void handleRandomPort()}
        loading={enableRandomPort.isPending}
      />
    </ItemContainer>
  );
}
