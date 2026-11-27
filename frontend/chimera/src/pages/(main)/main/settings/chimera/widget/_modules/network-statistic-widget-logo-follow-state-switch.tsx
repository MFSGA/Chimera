import { useSetting } from '@chimera/interface';
import { Switch } from '@chimera/ui/switch';
import { useLockFn } from '@/hooks/use-lock-fn';
import { m } from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../../_modules/settings-card';

export default function NetworkStatisticWidgetLogoFollowStateSwitch() {
  const logo = useSetting('network_statistic_widget_logo');

  const handleChange = useLockFn(async (followState: boolean) => {
    if (!logo.value) return;

    await logo.upsert({ ...logo.value, follow_state: followState });
  });

  return (
    <SettingsCard data-slot="network-statistic-widget-logo-follow-state-switch">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>
              {m.settings_chimera_network_statistic_widget_logo_follow_state_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_chimera_network_statistic_widget_logo_follow_state_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            aria-label={m.settings_chimera_network_statistic_widget_logo_follow_state_label()}
            checked={Boolean(logo.value?.follow_state)}
            disabled={logo.value === undefined}
            onCheckedChange={handleChange}
            loading={logo.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
