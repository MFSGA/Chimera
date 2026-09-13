import { BaseCard, Expand, MenuItem, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import {
  Button,
  InputAdornment,
  List,
  ListItem,
  ListItemText,
  TextField,
} from '@mui/material';
import { useAtom } from 'jotai';
import { useEffect, useState } from 'react';
import { useLanguage } from '@/components/providers/language-provider';
import {
  DEFAULT_COLOR,
  ThemeMode,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider';
import { useUiSwitch } from '@/features/ui-switch/use-ui-switch';
import * as m from '@/paraglide/messages';
import type { Locale } from '@/paraglide/runtime';
import { atomIsDrawerOnlyIcon } from '@/store';
import { languageOptions } from '@/utils/language';

const commonSx = {
  width: 128,
};

const HEX_COLOR_PATTERN = /^#[0-9a-f]{3}(?:[0-9a-f]{3})?$/i;

const isHexColor = (value: string | null | undefined) =>
  typeof value === 'string' && HEX_COLOR_PATTERN.test(value);

const toColorInputValue = (value: string | null | undefined) => {
  if (typeof value !== 'string') return DEFAULT_COLOR;
  if (/^#[0-9a-f]{6}$/i.test(value)) return value;
  if (/^#[0-9a-f]{3}$/i.test(value)) {
    const [r, g, b] = value.slice(1);
    return `#${r}${r}${g}${g}${b}${b}`;
  }
  return DEFAULT_COLOR;
};

const LanguageSwitch = () => {
  const { setLanguage, language: currentLocale } = useLanguage();

  return (
    <MenuItem
      label={m.settings_user_interface_language_label()}
      selectSx={commonSx}
      options={languageOptions}
      selected={currentLocale || 'en'}
      onSelected={(value) => setLanguage(value as Locale)}
    />
  );
};

const ThemeSwitch = () => {
  const themeOptions = {
    dark: m.settings_user_interface_theme_mode_dark(),
    light: m.settings_user_interface_theme_mode_light(),
    system: m.settings_user_interface_theme_mode_system(),
  };

  const { themeMode, setThemeMode } = useExperimentalThemeContext();

  return (
    <MenuItem
      id="user-interface-theme-mode"
      label={m.settings_user_interface_theme_mode_label()}
      selectSx={commonSx}
      options={themeOptions}
      selected={themeMode || ThemeMode.SYSTEM}
      onSelected={(value) => void setThemeMode(value as ThemeMode)}
    />
  );
};

const ThemeColor = () => {
  const { themeColor, setThemeColor } = useExperimentalThemeContext();
  const [value, setValue] = useState(themeColor);

  useEffect(() => {
    setValue(themeColor);
  }, [themeColor]);

  return (
    <>
      <ListItem sx={{ pl: 0, pr: 0 }}>
        <ListItemText primary={m.settings_user_interface_theme_color_label()} />

        <TextField
          size="small"
          sx={commonSx}
          value={value ?? DEFAULT_COLOR}
          error={!isHexColor(value ?? DEFAULT_COLOR)}
          onBlur={() => {
            if (!isHexColor(value ?? DEFAULT_COLOR)) {
              setValue(themeColor);
            }
          }}
          onChange={(event) => setValue(event.target.value)}
          slotProps={{
            htmlInput: {
              'aria-label': m.settings_user_interface_theme_color_label(),
              maxLength: 7,
            },
            input: {
              endAdornment: (
                <InputAdornment position="end">
                  <input
                    type="color"
                    aria-label={m.settings_user_interface_theme_color_label()}
                    className="size-6 cursor-pointer border-0 bg-transparent p-0"
                    value={toColorInputValue(value)}
                    onChange={(event) => setValue(event.target.value)}
                  />
                </InputAdornment>
              ),
            },
          }}
        />
      </ListItem>

      <Expand open={themeColor !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => {
              if (isHexColor(value)) {
                void setThemeColor(value);
              } else {
                // 如果输入的不是有效的十六进制颜色，则恢复为之前的值
                setValue(themeColor);
              }
            }}
          >
            {m.common_apply()}
          </Button>
        </div>
      </Expand>
    </>
  );
};

const ExperimentalSwitch = () => {
  const { switchToMain, isPending } = useUiSwitch();

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary="Switch to Experimental UI" />

      <Button variant="contained" loading={isPending} onClick={switchToMain}>
        Continue
      </Button>
    </ListItem>
  );
};

export const SettingChimerauUI = () => {
  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon);

  return (
    <BaseCard label={m.settings_user_interface_title()}>
      <List disablePadding>
        <LanguageSwitch />

        <ThemeSwitch />

        <ThemeColor />

        <SwitchItem
          label={m.settings_user_interface_icon_nav_label()}
          checked={onlyIcon}
          onChange={() => setOnlyIcon(!onlyIcon)}
        />

        <ExperimentalSwitch />
      </List>
    </BaseCard>
  );
};

export default SettingChimerauUI;
