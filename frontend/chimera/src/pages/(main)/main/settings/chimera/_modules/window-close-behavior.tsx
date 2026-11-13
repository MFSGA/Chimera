import {
  useSetting,
  type WindowCloseBehavior,
  type WindowCloseOverride,
  type WindowCloseSettings,
  type WindowCloseSettingsPatch_Serialize,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@chimera/ui/dropdown-menu';
import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

const DEFAULT_SETTINGS = {
  global: 'destroy',
  main: 'inherit',
  editor: 'inherit',
  tray_menu: 'inherit',
} satisfies WindowCloseSettings;

type WindowKind = 'main' | 'editor';

const changeOf = (
  kind: WindowKind,
  behavior: WindowCloseOverride,
): WindowCloseSettingsPatch_Serialize => {
  switch (kind) {
    case 'main':
      return { main: behavior };
    case 'editor':
      return { editor: behavior };
  }
};

function WindowCloseRow<V extends string>({
  slot,
  label,
  value,
  messages,
  disabled,
  onChange,
}: {
  slot: string;
  label: string;
  value: V;
  messages: Record<V, string>;
  disabled: boolean;
  onChange: (next: V) => void;
}) {
  return (
    <SettingsCard data-slot={slot}>
      <DropdownMenu align="end">
        <DropdownMenuTrigger asChild>
          <SettingsCardContent data-slot={`${slot}-trigger`} asChild>
            <Button
              disabled={disabled}
              className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base"
            >
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>{label}</ItemLabelText>
                  <ItemLabelDescription>{messages[value]}</ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>
        <DropdownMenuContent sideOffset={-16} alignOffset={16}>
          {Object.entries(messages).map(([key, message]) => (
            <DropdownMenuCheckboxItem
              checked={key === value}
              key={key}
              onSelect={() => onChange(key as V)}
            >
              {message as string}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}

/**
 * Follows ref's per-kind override editor, with Chimera's retained Legacy
 * window sharing the main policy. No tray-menu control is shown until the
 * WebView tray menu uses WindowManager too.
 */
export default function WindowCloseBehaviorCards() {
  const { value, upsert, isPending } = useSetting('window_close');
  const settings = { ...DEFAULT_SETTINGS, ...value };

  const behaviors = {
    destroy: m.settings_chimera_window_close_destroy(),
    hide: m.settings_chimera_window_close_hide(),
  } satisfies Record<WindowCloseBehavior, string>;

  const overrides = {
    inherit: m.settings_chimera_window_close_inherit(),
    ...behaviors,
  } satisfies Record<WindowCloseOverride, string>;

  const kinds: { kind: WindowKind; label: string }[] = [
    { kind: 'main', label: m.settings_chimera_window_close_main() },
    { kind: 'editor', label: m.settings_chimera_window_close_editor() },
  ];

  return (
    <>
      <WindowCloseRow
        slot="window-close-behavior-global"
        label={m.settings_chimera_window_close_global()}
        value={settings.global}
        messages={behaviors}
        disabled={!value || isPending}
        onChange={(global) => void upsert({ global })}
      />
      {kinds.map(({ kind, label }) => (
        <WindowCloseRow
          key={kind}
          slot={`window-close-behavior-${kind}`}
          label={label}
          value={settings[kind]}
          messages={overrides}
          disabled={!value || isPending}
          onChange={(behavior) => void upsert(changeOf(kind, behavior))}
        />
      ))}
      <SettingsCard data-slot="window-close-behavior-description-card">
        <SettingsCardContent>
          <p
            data-slot="window-close-behavior-description"
            className="text-on-surface-variant text-sm"
          >
            {m.settings_chimera_window_close_description()}
          </p>
        </SettingsCardContent>
      </SettingsCard>
    </>
  );
}
