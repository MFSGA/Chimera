import { useClashConfig } from '@chimera/interface';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
} from '@/components/settings/settings-card';
import { Switch } from '@/components/ui/switch';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const AllowLanSwitch = () => {
  const { query, upsert } = useClashConfig();

  return (
    <ItemContainer data-slot="allow-lan-switch-container">
      <ItemLabel>
        <ItemLabelText>
          {m.settings_clash_settings_allow_lan_label()}
        </ItemLabelText>
      </ItemLabel>
      <Switch
        checked={Boolean(query.data?.['allow-lan'])}
        loading={upsert.isPending}
        onCheckedChange={(checked) =>
          void upsert.mutateAsync({ 'allow-lan': checked }).catch((error) =>
            message(formatError(error), {
              title: m.common_error(),
              kind: 'error',
            }),
          )
        }
      />
    </ItemContainer>
  );
};
