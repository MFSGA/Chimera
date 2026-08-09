import { useSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function ProxyGuardSwitch() {
  const proxyGuard = useSetting('enable_proxy_guard');

  return (
    <ItemContainer data-slot="proxy-guard-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_proxy_guard_switch_label()}
        </ItemLabelText>
        <ItemLabelDescription>
          {m.settings_system_proxy_proxy_guard_switch_description()}
        </ItemLabelDescription>
      </ItemLabel>
      <Switch
        checked={Boolean(proxyGuard.value)}
        loading={proxyGuard.isPending}
        onCheckedChange={(checked) => void proxyGuard.upsert(checked)}
      />
    </ItemContainer>
  );
}
