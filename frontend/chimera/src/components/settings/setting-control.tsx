import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { Switch } from '@/components/ui/switch';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from './settings-card';

export function SwitchCard({
  label,
  description,
  checked,
  loading,
  disabled,
  onCheckedChange,
}: {
  label: string;
  description?: string;
  checked: boolean;
  loading?: boolean;
  disabled?: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <SettingsCard>
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>{label}</ItemLabelText>
            {description && (
              <ItemLabelDescription>{description}</ItemLabelDescription>
            )}
          </ItemLabel>
          <Switch
            checked={checked}
            loading={loading}
            disabled={disabled}
            onCheckedChange={onCheckedChange}
          />
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}

export function SelectorCard<T extends string>({
  label,
  current,
  options,
  onSelect,
}: {
  label: string;
  current: T;
  options: Record<T, string>;
  onSelect: (value: T) => void;
}) {
  return (
    <SettingsCard>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <SettingsCardContent asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>{label}</ItemLabelText>
                  <ItemLabelDescription>
                    {options[current]}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent align="end" sideOffset={-16} alignOffset={16}>
          {Object.entries<string>(options).map(([value, optionLabel]) => (
            <DropdownMenuCheckboxItem
              checked={current === value}
              key={value}
              onSelect={() => onSelect(value as T)}
            >
              {optionLabel}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}
