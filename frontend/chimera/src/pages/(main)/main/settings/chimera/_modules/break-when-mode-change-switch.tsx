import { useClashSetting } from '@chimera/interface';
import { Switch } from '@chimera/ui/switch';
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

export default function BreakWhenModeChangeSwitch() {
  const breakWhenModeChange = useClashSetting('break_connection');

  const handleChange = useLockFn(async () => {
    try {
      await breakWhenModeChange.upsert({
        on_mode_change: !breakWhenModeChange.value?.on_mode_change,
      });
    } catch (error) {
      message(
        `Update break when mode change failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      );
    }
  });

  return (
    <SettingsCard data-slot="break-when-mode-change-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="break-when-mode-change-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_chimera_enhance_break_when_mode_change_label()}
            </ItemLabelText>

            <ItemLabelDescription>
              {m.settings_chimera_enhance_break_when_mode_change_description()}
            </ItemLabelDescription>
          </ItemLabel>

          <Switch
            checked={Boolean(breakWhenModeChange.value?.on_mode_change)}
            onCheckedChange={handleChange}
            loading={breakWhenModeChange.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
