import { Button } from '@chimera/ui/button';
import {
  LauncherMenu,
  LauncherMenuChipButton,
  LauncherMenuGroup,
  LauncherMenuIconButton,
  LauncherMenuRow,
} from '@chimera/ui/launcher-menu';
import { NumberStepper } from '@chimera/ui/number-stepper';
import { Popover, PopoverContent, PopoverTrigger } from '@chimera/ui/popover';
import { ScrollArea } from '@chimera/ui/scroll-area';
import { Switch } from '@chimera/ui/switch';
import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded';
import SwapHorizRounded from '~icons/material-symbols/swap-horiz-rounded';
import TuneRounded from '~icons/material-symbols/tune-rounded';
import { motion, useReducedMotion } from 'motion/react';
import { useEffect, useId, useState } from 'react';
import * as m from '@/paraglide/messages';
import { useDashboardContext } from './provider';
import {
  DEFAULT_WIDGET_CONFIGS,
  getWidgetConfig,
  WIDGET_CONFIG_NUMBER_RANGES,
  WidgetId,
  type WidgetConfig,
} from './widget-config';
import { ConfigGroup, ConfigRow } from './widget-config-row';

type ChoiceOption<T extends string> = { value: T; label: string };

function Choice<T extends string>({
  field,
  label,
  value,
  options,
  disabled,
  onChange,
}: {
  field:
    'speed-unit' | 'shortcut-layout' | 'shortcut-buttons' | 'shortcut-order';
  label: string;
  value: T;
  options: ChoiceOption<T>[];
  disabled: boolean;
  onChange: (value: T) => void;
}) {
  const selected = options.find((option) => option.value === value);

  if (options.length === 2) {
    const other = options.find((option) => option.value !== value);
    return (
      <ConfigRow label={label}>
        <LauncherMenuChipButton
          data-slot="widget-config-choice"
          data-config-field={field}
          aria-label={`${label}: ${selected?.label ?? value}`}
          disabled={disabled || !other}
          onClick={() => other && onChange(other.value)}
        >
          <span className="truncate">{selected?.label ?? value}</span>
          <SwapHorizRounded className="size-4 shrink-0" aria-hidden="true" />
        </LauncherMenuChipButton>
      </ConfigRow>
    );
  }

  return (
    <ConfigRow label={label}>
      <select
        className="bg-surface-variant text-on-surface max-w-40 rounded-xl px-2 py-1.5 text-xs"
        data-slot="widget-config-choice"
        data-config-field={field}
        aria-label={label}
        value={value}
        disabled={disabled}
        onChange={(event) => {
          const option = options.find(
            (item) => item.value === event.target.value,
          );
          if (option) onChange(option.value);
        }}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </ConfigRow>
  );
}

function Toggle({
  label,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  checked: boolean;
  disabled: boolean;
  onChange: (value: boolean) => void;
}) {
  const id = useId();
  return (
    <ConfigRow label={label} htmlFor={id}>
      <Switch
        id={id}
        className="h-6 w-11"
        checked={checked}
        disabled={disabled}
        onCheckedChange={onChange}
      />
    </ConfigRow>
  );
}

function Stepper({
  label,
  value,
  range,
  disabled,
  onChange,
}: {
  label: string;
  value: number;
  range: readonly [number, number];
  disabled: boolean;
  onChange: (value: number) => void;
}) {
  return (
    <div data-slot="widget-config-stepper">
      <NumberStepper
        variant="filled"
        layout="inline"
        label={label}
        value={value}
        min={range[0]}
        max={range[1]}
        decrementLabel={`${m.dashboard_widget_config_decrease()} ${label}`}
        incrementLabel={`${m.dashboard_widget_config_increase()} ${label}`}
        disabled={disabled}
        onChange={onChange}
      />
    </div>
  );
}

function ConfigFieldRows({
  config,
  disabled,
  onChange,
}: {
  config: WidgetConfig;
  disabled: boolean;
  onChange: (config: WidgetConfig) => void;
}) {
  switch (config.type) {
    case WidgetId.TrafficDown:
    case WidgetId.TrafficUp:
      return (
        <ConfigGroup>
          <Toggle
            label={m.dashboard_widget_sparkline_config_chart()}
            checked={config.showChart}
            disabled={disabled}
            onChange={(showChart) => onChange({ ...config, showChart })}
          />
          <Toggle
            label={m.dashboard_widget_traffic_config_total()}
            checked={config.showTotal}
            disabled={disabled}
            onChange={(showTotal) => onChange({ ...config, showTotal })}
          />
          <Choice
            field="speed-unit"
            label={m.dashboard_widget_traffic_config_speed_unit()}
            value={config.unit}
            disabled={disabled}
            options={[
              { value: 'bytes', label: 'B/s' },
              { value: 'bits', label: 'bit/s' },
            ]}
            onChange={(unit) => onChange({ ...config, unit })}
          />
        </ConfigGroup>
      );
    case WidgetId.Connections:
    case WidgetId.Memory:
      return (
        <ConfigGroup>
          <Toggle
            label={m.dashboard_widget_sparkline_config_chart()}
            checked={config.showChart}
            disabled={disabled}
            onChange={(showChart) => onChange({ ...config, showChart })}
          />
          <Stepper
            label={m.dashboard_widget_sparkline_config_samples()}
            value={config.samples}
            range={WIDGET_CONFIG_NUMBER_RANGES.samples}
            disabled={disabled || !config.showChart}
            onChange={(samples) => onChange({ ...config, samples })}
          />
        </ConfigGroup>
      );
    case WidgetId.ProxyShortcuts:
      return (
        <ConfigGroup>
          <Choice
            field="shortcut-layout"
            label={m.dashboard_widget_proxy_shortcuts_config_layout()}
            value={config.layout}
            disabled={disabled}
            options={[
              {
                value: 'equal',
                label: m.dashboard_widget_proxy_shortcuts_config_equal(),
              },
              {
                value: 'flex',
                label: m.dashboard_widget_proxy_shortcuts_config_flex(),
              },
            ]}
            onChange={(layout) => onChange({ ...config, layout })}
          />
          <Choice
            field="shortcut-buttons"
            label={m.dashboard_widget_proxy_shortcuts_config_buttons()}
            value={config.buttons}
            disabled={disabled}
            options={[
              {
                value: 'both',
                label: m.dashboard_widget_proxy_shortcuts_config_both(),
              },
              {
                value: 'system',
                label: m.dashboard_widget_proxy_shortcuts_config_system(),
              },
              { value: 'tun', label: 'TUN' },
            ]}
            onChange={(buttons) => onChange({ ...config, buttons })}
          />
          {config.buttons === 'both' && (
            <Choice
              field="shortcut-order"
              label={m.dashboard_widget_proxy_shortcuts_config_order()}
              value={config.order}
              disabled={disabled}
              options={[
                {
                  value: 'system-first',
                  label:
                    m.dashboard_widget_proxy_shortcuts_config_system_first(),
                },
                {
                  value: 'tun-first',
                  label: m.dashboard_widget_proxy_shortcuts_config_tun_first(),
                },
              ]}
              onChange={(order) => onChange({ ...config, order })}
            />
          )}
        </ConfigGroup>
      );
    case WidgetId.CoreShortcuts:
      return (
        <ConfigGroup>
          <Toggle
            label={m.dashboard_widget_core_shortcuts_config_version()}
            checked={config.showVersion}
            disabled={disabled}
            onChange={(showVersion) => onChange({ ...config, showVersion })}
          />
        </ConfigGroup>
      );
  }
}

export function WidgetConfigSaveStatus() {
  const { saveStatus, configLoading, configReadError, retryConfig } =
    useDashboardContext();
  const failed = configReadError || saveStatus.state === 'error';

  if (!configLoading && saveStatus.state !== 'saving' && !failed) return null;

  return (
    <div
      className="text-on-surface-variant flex items-center justify-between gap-2 text-xs"
      data-slot="widget-config-save-status"
      data-save-state={
        configLoading
          ? 'loading'
          : configReadError
            ? 'load-error'
            : saveStatus.state
      }
    >
      <span
        role="status"
        aria-live="polite"
        className={failed ? 'text-error' : undefined}
      >
        {configLoading
          ? m.dashboard_widget_config_loading()
          : configReadError
            ? m.dashboard_widget_config_load_failed()
            : saveStatus.state === 'saving'
              ? m.dashboard_widget_config_saving()
              : m.dashboard_widget_config_save_failed()}
      </span>
      {failed && (
        <Button
          className="h-7 min-w-0 px-2 text-xs"
          disabled={configLoading || saveStatus.state === 'saving'}
          onClick={retryConfig}
        >
          {m.dashboard_widget_config_retry()}
        </Button>
      )}
    </div>
  );
}

export default function WidgetConfigMenu({
  id,
  type,
  isActive = true,
}: {
  id: string;
  type: WidgetId;
  isActive?: boolean;
}) {
  const { configs, saveConfig, saveStatus, configLoading, configReadError } =
    useDashboardContext();
  const config = getWidgetConfig(configs, id, type);
  const disabled =
    configLoading || configReadError || saveStatus.state === 'saving';
  const titleId = useId();
  const reducedMotion = useReducedMotion();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    if (!isActive) setOpen(false);
  }, [isActive]);

  return (
    <Popover open={isActive && open} onOpenChange={setOpen}>
      <Tooltip>
        <TooltipTrigger asChild>
          <PopoverTrigger asChild>
            <motion.button
              type="button"
              aria-label={m.dashboard_widget_config_title()}
              data-slot="widget-config-trigger"
              data-widget-type={type}
              data-widget-id={id}
              className="border-outline/30 bg-surface text-on-surface hover:bg-surface-variant data-[state=open]:bg-primary-container data-[state=open]:text-on-primary-container focus-visible:ring-primary pointer-events-auto absolute -bottom-1 left-1/2 z-20 grid h-6 w-9 -translate-x-1/2 cursor-pointer place-items-center rounded-full border shadow-sm outline-none focus-visible:ring-2"
              onPointerDown={(event) => event.stopPropagation()}
              onKeyDown={(event) => event.stopPropagation()}
              onClick={(event) => event.stopPropagation()}
              whileHover={reducedMotion ? undefined : { scale: 1.06 }}
              whileTap={reducedMotion ? undefined : { scale: 0.94 }}
            >
              <TuneRounded className="size-3.5" aria-hidden="true" />
            </motion.button>
          </PopoverTrigger>
        </TooltipTrigger>
        <TooltipContent>{m.dashboard_widget_config_title()}</TooltipContent>
      </Tooltip>
      <PopoverContent
        aria-labelledby={titleId}
        side="top"
        sideOffset={8}
        className="w-80 overflow-visible border-0 bg-transparent shadow-none"
        onPointerDown={(event) => event.stopPropagation()}
        onKeyDown={(event) => event.stopPropagation()}
        onClick={(event) => event.stopPropagation()}
      >
        <LauncherMenu
          className="min-h-0 flex-auto"
          data-slot="widget-config-menu"
        >
          <LauncherMenuGroup data-slot="widget-config-header">
            <LauncherMenuRow className="justify-between">
              <h2 id={titleId} className="text-sm font-medium">
                {m.dashboard_widget_config_title()}
              </h2>
              <LauncherMenuIconButton
                data-slot="widget-config-reset"
                aria-label={m.dashboard_widget_config_reset()}
                title={m.dashboard_widget_config_reset()}
                disabled={disabled}
                onClick={() => saveConfig(id, DEFAULT_WIDGET_CONFIGS[type])}
              >
                <RestartAltRounded className="size-5" aria-hidden="true" />
              </LauncherMenuIconButton>
            </LauncherMenuRow>
          </LauncherMenuGroup>

          <ScrollArea
            className="max-h-[min(24rem,65vh)] min-h-0 flex-auto rounded-3xl"
            data-slot="widget-config-scroll-area"
          >
            <div data-slot="widget-config-fields">
              <ConfigFieldRows
                config={config}
                disabled={disabled}
                onChange={(next) => saveConfig(id, next)}
              />
            </div>
          </ScrollArea>
          {(configLoading ||
            configReadError ||
            saveStatus.state !== 'saved') && (
            <LauncherMenuGroup data-slot="widget-config-footer">
              <LauncherMenuRow className="py-3">
                <WidgetConfigSaveStatus />
              </LauncherMenuRow>
            </LauncherMenuGroup>
          )}
        </LauncherMenu>
      </PopoverContent>
    </Popover>
  );
}
