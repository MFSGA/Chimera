import { Switch } from '@/components/ui/switch';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { useSystemServiceMode } from '@/features/system-service/use-system-service-mode';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
} from '../../_modules/settings-card';

export default function SystemServiceSwitch() {
  const serviceMode = useSystemServiceMode();

  return (
    <ItemContainer data-slot="system-service-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_system_proxy_service_mode_label()}
        </ItemLabelText>
        <ItemLabelDescription>
          {m.settings_system_proxy_service_mode_description()}
        </ItemLabelDescription>
      </ItemLabel>
      <Tooltip>
        <TooltipTrigger asChild>
          <div data-slot="system-service-switch-trigger-wrapper">
            <Switch
              disabled={!serviceMode.isInstalled}
              checked={serviceMode.value}
              loading={serviceMode.isPending}
              onCheckedChange={serviceMode.toggle}
            />
          </div>
        </TooltipTrigger>

        {!serviceMode.isInstalled && (
          <TooltipContent>
            <span>
              {m.settings_system_proxy_service_mode_disabled_tooltip()}
            </span>
          </TooltipContent>
        )}
      </Tooltip>
    </ItemContainer>
  );
}
