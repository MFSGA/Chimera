import { useSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function AutoLaunchSwitch() {
  const autoLaunch = useSetting('enable_auto_launch');

  return (
    <ItemContainer data-slot="auto-launch-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_auto_launch_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
        checked={Boolean(autoLaunch.value)}
        loading={autoLaunch.isPending}
        onCheckedChange={(checked) => void autoLaunch.upsert(checked)}
      />
    </ItemContainer>
  );
}
