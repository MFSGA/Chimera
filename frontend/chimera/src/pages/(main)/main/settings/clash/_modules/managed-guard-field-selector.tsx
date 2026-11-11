import { useClashSetting, type ManageableField } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@chimera/ui/dropdown-menu';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
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

type ManagedGuardField = 'unified-delay' | 'tcp-concurrent';
type ManagedGuardOption = 'unmanaged' | 'enabled' | 'disabled';

const FIELD_LABELS = {
  'unified-delay': m.settings_clash_settings_unified_delay_label,
  'tcp-concurrent': m.settings_clash_settings_tcp_concurrent_label,
} satisfies Record<ManagedGuardField, () => string>;

const OPTION_LABELS = {
  unmanaged: m.settings_clash_settings_managed_guard_field_unmanaged,
  enabled: m.settings_clash_settings_managed_guard_field_enabled,
  disabled: m.settings_clash_settings_managed_guard_field_disabled,
} satisfies Record<ManagedGuardOption, () => string>;

const toOption = (field: ManageableField<boolean>): ManagedGuardOption => {
  if (field.kind === 'unmanaged') {
    return 'unmanaged';
  }

  return field.value ? 'enabled' : 'disabled';
};

const toField = (option: ManagedGuardOption): ManageableField<boolean> =>
  option === 'unmanaged'
    ? { kind: 'unmanaged' }
    : { kind: 'managed', value: option === 'enabled' };

export default function ManagedGuardFieldSelector({
  field,
}: {
  field: ManagedGuardField;
}) {
  const overrides = useClashSetting('overrides');
  const current = overrides.value?.[field];
  const selected = current ? toOption(current) : undefined;

  const handleChange = useLockFn(async (option: ManagedGuardOption) => {
    try {
      await overrides.upsert({
        [field]: toField(option),
      });
    } catch (error) {
      message(`Update ${field} failed!\nError: ${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });

  return (
    <SettingsCard data-slot="managed-guard-field-selector-card">
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent
            data-slot="managed-guard-field-selector-trigger"
            asChild
          >
            <Button
              className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
              disabled={!overrides.value}
            >
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>{FIELD_LABELS[field]()}</ItemLabelText>
                  <ItemLabelDescription>
                    {selected ? OPTION_LABELS[selected]() : null}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(OPTION_LABELS).map(([option, label]) => (
            <DropdownMenuCheckboxItem
              checked={selected === option}
              key={option}
              onSelect={() => void handleChange(option as ManagedGuardOption)}
            >
              {label()}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}
