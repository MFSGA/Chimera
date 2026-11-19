import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@chimera/ui/select';
import { ConfigRow } from './widget-config-row';

/**
 * Ref widget configuration select. Dynamic profile/provider reference fields
 * can be added here as their corresponding Chimera query interfaces migrate.
 */
export function WidgetOptionSelect({
  label,
  value,
  options,
  disabled,
  onChange,
  field,
}: {
  label: string;
  value: string;
  options: { value: string; label: string; disabled?: boolean }[];
  disabled: boolean;
  onChange: (value: string) => void;
  /** Stable dashboard E2E locator retained from Chimera's old native select. */
  field?: string;
}) {
  const selected = options.find((option) => option.value === value);

  return (
    <ConfigRow label={label}>
      <Select
        variant="chip"
        value={value}
        disabled={disabled}
        onValueChange={onChange}
      >
        <SelectTrigger
          aria-label={label}
          data-slot="widget-config-choice"
          data-config-field={field}
        >
          <SelectValue>{selected?.label ?? value}</SelectValue>
        </SelectTrigger>
        <SelectContent>
          {options.map((option) => (
            <SelectItem
              key={option.value}
              value={option.value}
              disabled={option.disabled}
            >
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </ConfigRow>
  );
}
