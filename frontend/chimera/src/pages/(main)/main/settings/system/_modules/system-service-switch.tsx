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
  const serviceMode = useSetting('enable_service_mode');

  const { query } = useSystemService();

  const notInstalled = query.data?.status === 'not_installed';
  const compatKind = query.data?.compat.kind;
  const compatBlocked =
    compatKind === 'incompatible' || compatKind === 'unparsable';
  const disabled = notInstalled || (compatBlocked && !serviceMode.value);
  const hint = compatBlocked
    ? m.agent_finding_service_mode_inconsistent()
    : notInstalled
      ? m.settings_system_proxy_service_mode_disabled_tooltip()
      : null;

  const handleServiceMode = useLockFn(async () => {
    try {
      await serviceMode.upsert(!serviceMode.value);
    } catch (error) {
      message(
        `Activation Service Mode failed!\n Error: ${formatError(error)}`,
        {
          title: 'Error',
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
              checked={Boolean(serviceMode.value)}
              onCheckedChange={handleServiceMode}
              loading={serviceMode.isPending}
              disabled={disabled}
            />
          </div>
        </TooltipTrigger>

        {hint && (
          <TooltipContent>
            <span>{hint}</span>
          </TooltipContent>
        )}
      </Tooltip>
    </ItemContainer>
  );
}
