import { useSetting } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function LightenAnimationEffectsSwitch() {
  const lightenAnimationEffects = useSetting('lighten_animation_effects');

  const handleChange = useLockFn(async () => {
    try {
      await lightenAnimationEffects.upsert(!lightenAnimationEffects.value);
    } catch (error) {
      message(
        `Update lighten animation effects failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
          kind: 'error',
        },
      );
    }
  });

  return (
    <SettingsCard data-slot="lighten-animation-effects-switch">
      <SettingsCardContent>
        <ItemContainer data-slot="lighten-animation-effects-switch-container">
          <ItemLabel>
            <ItemLabelText>
              {m.settings_chimera_lighten_animations_label()}
            </ItemLabelText>
          </ItemLabel>

          <Switch
            checked={Boolean(lightenAnimationEffects.value)}
            onCheckedChange={handleChange}
            loading={lightenAnimationEffects.isPending}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
