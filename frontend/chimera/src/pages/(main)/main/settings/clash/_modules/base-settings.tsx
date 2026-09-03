import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '@/components/settings/settings-card';
import { Switch } from '@/components/ui/switch';
import { useClashBaseSettings } from '@/features/clash-settings/use-clash-base-settings';
import * as m from '@/paraglide/messages';

export const AllowLanSwitch = () => {
  const { allowLan, isPending, setAllowLan } = useClashBaseSettings();

  return (
    <ItemContainer data-slot="allow-lan-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_allow_lan_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
        checked={allowLan}
        loading={isPending}
        onCheckedChange={(checked) => void setAllowLan(checked)}
      />
    </ItemContainer>
  );
};
