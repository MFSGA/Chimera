import { Switch } from '@/components/ui/switch';
import { useClashBaseSettings } from '@/features/clash-settings/use-clash-base-settings';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function IPv6Switch() {
  const { ipv6, isPending, setIPv6 } = useClashBaseSettings();

  return (
    <ItemContainer data-slot="ipv6-switch-container">
      <ItemLabel>
        <ItemLabelText>{m.settings_clash_settings_ipv6_label()}</ItemLabelText>
      </ItemLabel>

      <Switch
        checked={ipv6}
        onCheckedChange={(checked) => void setIPv6(checked)}
        loading={isPending}
      />
    </ItemContainer>
  );
}
