import { useHotkeyFunctions, useHotkeys } from '@chimera/interface';
import CloseRounded from '~icons/material-symbols/close-rounded';
import EditOutlineRounded from '~icons/material-symbols/edit-outline-rounded';
import ErrorRounded from '~icons/material-symbols/error-rounded';
import { isEqual } from 'lodash-es';
import { useEffect, useRef, useState } from 'react';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { parseHotkey } from '@/utils/parse-hotkey';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

type HotkeyMap = Record<string, string[]>;

const HotkeyItem = ({
  keys,
  onChange,
}: {
  keys: string[];
  onChange: (keys: string[]) => Promise<void> | void;
}) => {
  const [isListening, setIsListening] = useState(false);
  const [currentKeys, setCurrentKeys] = useState(keys);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setCurrentKeys(keys);
  }, [keys]);

  const handleSave = useLockFn(async (newKeys: string[]) => {
    if (isEqual(newKeys, keys)) {
      return true;
    }

    try {
      await onChange(newKeys);
      return true;
    } catch (error) {
      setCurrentKeys(keys);
      message(formatError(error), { kind: 'error' });
      return false;
    }
  });

  const handleStartListening = () => {
    setIsListening(true);
    setCurrentKeys([]);
    inputRef.current?.focus();
  };

  const handleStopListening = useLockFn(async () => {
    setIsListening(false);
    if (currentKeys.length > 0) {
      await handleSave(currentKeys);
    }
  });

  const handleKeyDown = (event: React.KeyboardEvent<HTMLInputElement>) => {
    event.preventDefault();
    event.stopPropagation();

    const key = parseHotkey(event.key);
    if (key === 'UNIDENTIFIED') {
      return;
    }

    setCurrentKeys((previous) => [...new Set([...previous, key])]);
  };

  const handleClear = (event: React.MouseEvent) => {
    event.stopPropagation();
    setCurrentKeys([]);
    onChange([]);
  };

  return (
    <div className="flex items-center gap-2">
      <div className="border-input relative flex min-h-8 flex-wrap items-center gap-1 px-1">
        {currentKeys.map((key) => (
          <kbd
            key={key}
            className="bg-surface-variant text-on-surface rounded-sm px-1.5 py-0.5 text-xs"
          >
            {key}
          </kbd>
        ))}

        {currentKeys.length === 0 ? (
          isListening ? (
            <span className="text-on-surface animate-pulse text-xs">
              {m.settings_chimera_hotkey_press_key()}
            </span>
          ) : (
            <span className="text-on-surface text-xs">
              {m.settings_chimera_hotkey_no_key()}
            </span>
          )
        ) : null}
      </div>

      <Button
        icon
        onClick={isListening ? handleStopListening : handleStartListening}
        className="relative size-8 shrink-0"
        variant="raised"
      >
        {isListening ? <ErrorRounded /> : <EditOutlineRounded />}
        <input
          ref={inputRef}
          type="text"
          className="absolute inset-0 cursor-pointer opacity-0"
          onBlur={handleStopListening}
          onKeyDown={handleKeyDown}
          onKeyUp={handleStopListening}
        />
      </Button>

      <Button
        icon
        onClick={handleClear}
        className="size-8 shrink-0"
        variant="raised"
      >
        <CloseRounded />
      </Button>
    </div>
  );
};

export default function HotkeyManager() {
  const [hotkeyMap, setHotkeyMap] = useState<HotkeyMap>({});
  const { data: supportedFuncs = [] } = useHotkeyFunctions();
  const { data: hotkeyStrings = [], mutate: patchHotkeys } = useHotkeys();

  useEffect(() => {
    const map: HotkeyMap = {};

    hotkeyStrings.forEach((text) => {
      const [func, key] = text.split(',').map((part) => part.trim());
      if (!func || !key) {
        return;
      }

      map[func] = key.split('+').map((part) => (part === 'PLUS' ? '+' : part));
    });

    setHotkeyMap((previous) => (isEqual(previous, map) ? previous : map));
  }, [hotkeyStrings]);

  const saveHotkeys = useLockFn(async (newMap: HotkeyMap) => {
    const hotkeys = Object.entries(newMap)
      .filter(([, keys]) => keys && keys.length > 0)
      .map(([func, keys]) => {
        const key = keys
          .map((part) => (part === '+' ? 'PLUS' : part))
          .join('+');
        return `${func},${key}`;
      });

    await patchHotkeys(hotkeys);
  });

  const handleChange = useLockFn(async (func: string, newKeys: string[]) => {
    const updated = { ...hotkeyMap, [func]: newKeys };
    await saveHotkeys(updated);
    setHotkeyMap(updated);
  });

  const messages = {
    open_or_close_dashboard:
      m.settings_chimera_hotkey_open_or_close_dashboard(),
    clash_mode_rule: m.settings_chimera_hotkey_clash_mode_rule(),
    clash_mode_global: m.settings_chimera_hotkey_clash_mode_global(),
    clash_mode_direct: m.settings_chimera_hotkey_clash_mode_direct(),
    clash_mode_script: m.settings_chimera_hotkey_clash_mode_script(),
    toggle_system_proxy: m.settings_chimera_hotkey_toggle_system_proxy(),
    enable_system_proxy: m.settings_chimera_hotkey_enable_system_proxy(),
    disable_system_proxy: m.settings_chimera_hotkey_disable_system_proxy(),
    toggle_tun_mode: m.settings_chimera_hotkey_toggle_tun_mode(),
    enable_tun_mode: m.settings_chimera_hotkey_enable_tun_mode(),
    disable_tun_mode: m.settings_chimera_hotkey_disable_tun_mode(),
  } satisfies Record<string, string>;

  return (
    <SettingsCard data-slot="hotkey-manager">
      <SettingsCardContent
        className="gap-4 py-4"
        data-slot="hotkey-manager-content"
      >
        {supportedFuncs.map((func) => (
          <ItemContainer key={func}>
            <ItemLabel>
              <ItemLabelText>
                {messages[func as keyof typeof messages] ?? func}
              </ItemLabelText>
            </ItemLabel>

            <HotkeyItem
              keys={hotkeyMap[func] ?? []}
              onChange={(keys) => handleChange(func, keys)}
            />
          </ItemContainer>
        ))}
      </SettingsCardContent>
    </SettingsCard>
  );
}
