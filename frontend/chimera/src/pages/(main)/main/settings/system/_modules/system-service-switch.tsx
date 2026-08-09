import { useSetting, useSystemService } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function SystemServiceSwitch() {
  const { query } = useSystemService();
  const serviceMode = useSetting('enable_service_mode');
  const isInstalled =
    query.data?.status === 'running' || query.data?.status === 'stopped';

  return (
    <ItemContainer data-slot="system-service-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_service_mode_label()}
        </ItemLabelText>
        {!isInstalled && (
          <ItemLabelDescription>
            {m.settings_system_proxy_service_mode_description()}
          </ItemLabelDescription>
        )}
      </ItemLabel>
      <Switch
        disabled={!isInstalled}
        checked={Boolean(serviceMode.value)}
        loading={serviceMode.isPending}
        onCheckedChange={(checked) => void serviceMode.upsert(checked)}
      />
    </ItemContainer>
  );
}
