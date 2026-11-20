import { useSetting } from '@chimera/interface';
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

/** Upstream's opt-in switch, adapted to Chimera's existing settings screen. */
export default function LocalIpProbeSwitch() {
  const probe = useSetting('enable_local_ip_probe');
  const toggle = useLockFn(async () => {
    try {
      await probe.upsert(!probe.value);
    } catch (error) {
      message(`Public IP detection setting failed: ${formatError(error)}`, {
        title: 'Error',
        kind: 'error',
      });
    }
  });

  return (
    <SettingsCard data-slot="local-ip-probe-switch">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>
              {m.settings_nyanpasu_local_ip_probe_label()}
            </ItemLabelText>
            <ItemLabelDescription>
              {m.settings_nyanpasu_local_ip_probe_description()}
            </ItemLabelDescription>
          </ItemLabel>
          <Switch
            aria-label={m.settings_nyanpasu_local_ip_probe_label()}
            checked={probe.value === true}
            disabled={probe.value === undefined}
            loading={probe.isPending}
            onCheckedChange={() => void toggle()}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
