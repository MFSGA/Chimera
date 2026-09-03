import { useSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import { useLockFn } from '@/hooks/use-lock-fn';
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

export default function BreakWhenProfileChangeSwitch() {
  const breakWhenProfileChange = useSetting('break_when_profile_change');

  const handleChange = useLockFn(async () => {
    try {
      await breakWhenProfileChange.upsert(!breakWhenProfileChange.value);
    } catch (error) {
      message(
        `Update break when profile change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      );
    }
  });

  return (
    <SettingsCard data-slot="break-when-profile-change-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="break-when-profile-change-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_chimera_enhance_break_when_profile_change_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_chimera_enhance_break_when_profile_change_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            checked={Boolean(breakWhenProfileChange.value)}
            onCheckedChange={handleChange}
            loading={breakWhenProfileChange.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
