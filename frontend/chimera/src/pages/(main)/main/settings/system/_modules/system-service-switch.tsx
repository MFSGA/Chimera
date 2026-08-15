import { useSetting, useSystemService } from '@chimera/interface';
import { Switch } from '@/components/ui/switch';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
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

  const handleServiceMode = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value);
    } catch (error) {
      message(
        `Activation Service Mode failed! \n Error: ${formatError(error)}`,
        {
          title: m.common_error(),
          kind: 'error',
        },
      );
    }
  });

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
              disabled={!isInstalled}
              checked={Boolean(serviceMode.value)}
              loading={serviceMode.isPending}
              onCheckedChange={handleServiceMode}
            />
          </div>
        </TooltipTrigger>

        {!isInstalled && (
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
