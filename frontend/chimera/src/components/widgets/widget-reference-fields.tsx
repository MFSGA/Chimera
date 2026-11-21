import {
  getRemoteSource,
  isConfigItem,
  useClashProxiesProvider,
  useClashRulesProvider,
  useProfile,
} from '@chimera/interface';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@chimera/ui/select';
import { Switch } from '@chimera/ui/switch';
import { useId } from 'react';
import * as m from '@/paraglide/messages';
import type { ProviderReference, SubscriptionTarget } from './widget-config';
import { ConfigRow } from './widget-config-row';
import {
  canToggleProviderReference,
  providerReferenceChoices,
} from './widget-reference-model';

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

function ProfileSelect({
  value,
  remoteOnly,
  includeCurrent,
  disabled,
  onChange,
}: {
  value: string | null;
  remoteOnly?: boolean;
  includeCurrent?: boolean;
  disabled: boolean;
  onChange: (value: string | null) => void;
}) {
  const { query } = useProfile({ without_helper_fn: true });
  const items = (query.data?.items ?? []).filter(
    (item) => isConfigItem(item) && (!remoteOnly || getRemoteSource(item)),
  );
  const options = [
    {
      value: '*',
      label: includeCurrent
        ? m.dashboard_widget_config_follow_current()
        : m.dashboard_widget_config_all_profiles(),
    },
    ...items.map((item) => ({ value: item.uid, label: item.name ?? item.uid })),
  ];
  if (value && !items.some((item) => item.uid === value)) {
    options.push({
      value,
      label: query.isSuccess
        ? m.dashboard_widget_config_missing_reference({ name: value })
        : value,
    });
  }
  return (
    <div className="space-y-2">
      <WidgetOptionSelect
        label={m.dashboard_widget_config_profile()}
        value={value ?? '*'}
        options={options}
        disabled={disabled || query.isPending || (query.isError && !query.data)}
        onChange={(next) => onChange(next === '*' ? null : next)}
      />
      {query.isError && (
        <p role="status" className="text-error text-xs">
          {m.dashboard_widget_config_references_failed()}
        </p>
      )}
    </div>
  );
}

export function SubscriptionTargetField({
  target,
  disabled,
  onChange,
}: {
  target: SubscriptionTarget;
  disabled: boolean;
  onChange: (target: SubscriptionTarget) => void;
}) {
  return (
    <ProfileSelect
      remoteOnly
      includeCurrent
      value={target.kind === 'fixed' ? target.profileUid : null}
      disabled={disabled}
      onChange={(profileUid) =>
        onChange(
          profileUid ? { kind: 'fixed', profileUid } : { kind: 'current' },
        )
      }
    />
  );
}

export function ReportProfileField({
  profileUid,
  disabled,
  onChange,
}: {
  profileUid: string | null;
  disabled: boolean;
  onChange: (uid: string | null) => void;
}) {
  return (
    <div className="space-y-2" data-slot="widget-report-profile-config">
      <ProfileSelect
        value={profileUid}
        disabled={disabled}
        onChange={onChange}
      />
      <p className="text-on-surface-variant text-xs">
        {m.dashboard_widget_config_profile_attribution()}
      </p>
    </div>
  );
}

export function ProviderReferencesField({
  resources,
  disabled,
  onChange,
}: {
  resources: ProviderReference[];
  disabled: boolean;
  onChange: (refs: ProviderReference[]) => void;
}) {
  const idPrefix = useId();
  const proxies = useClashProxiesProvider();
  const rules = useClashRulesProvider();
  const choices = providerReferenceChoices(
    resources,
    !proxies.isPending && !proxies.isError && proxies.data
      ? Object.keys(proxies.data)
      : null,
    !rules.isPending && !rules.isError && rules.data
      ? Object.keys(rules.data)
      : null,
  );
  return (
    <>
      <div
        className="text-on-surface-variant text-xs"
        data-slot="widget-provider-references-config"
      >
        {m.dashboard_widget_config_resources()}
      </div>

      {(proxies.isError || rules.isError) && (
        <p className="text-error text-xs">
          {m.dashboard_widget_config_references_failed()}
        </p>
      )}

      {choices.map(({ ref, availability }) => {
        const checked = resources.some(
          (entry) => entry.kind === ref.kind && entry.name === ref.name,
        );
        const id = `${idPrefix}-${ref.kind}-${ref.name}`;

        return (
          <ConfigRow
            key={JSON.stringify(ref)}
            htmlFor={id}
            label={
              <span title={ref.name} className="block truncate">
                {ref.kind === 'proxy'
                  ? m.dashboard_widget_config_proxy_providers()
                  : m.dashboard_widget_config_rule_providers()}
                : {ref.name}
                {availability === 'missing'
                  ? ` (${m.dashboard_widget_config_missing()})`
                  : ''}
              </span>
            }
          >
            <Switch
              id={id}
              disabled={
                disabled ||
                !canToggleProviderReference({ ref, availability }, checked)
              }
              checked={checked}
              onCheckedChange={(next) =>
                onChange(
                  next
                    ? [...resources, ref]
                    : resources.filter(
                        (entry) =>
                          entry.kind !== ref.kind || entry.name !== ref.name,
                      ),
                )
              }
            />
          </ConfigRow>
        );
      })}
    </>
  );
}
