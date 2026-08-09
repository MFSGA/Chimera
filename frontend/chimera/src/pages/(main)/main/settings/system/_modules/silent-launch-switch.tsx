import { useSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function SilentLaunchSwitch() {
  const silentStart = useSetting('enable_silent_start');

  return (
    <ItemContainer data-slot="silent-launch-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_silent_start_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
        checked={Boolean(silentStart.value)}
        loading={silentStart.isPending}
        onCheckedChange={(checked) => void silentStart.upsert(checked)}
      />
    </ItemContainer>
  );
}
