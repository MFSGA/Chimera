import { BaseCard, Expand, MenuItem, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { useAtom } from 'jotai';
import { MuiColorInput } from 'mui-color-input';
import { useEffect, useState } from 'react';
import { isHexColor } from 'validator';
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

        <MuiColorInput
          size="small"
          sx={commonSx}
          value={value ?? DEFAULT_COLOR}
          isAlphaHidden
          format="hex"
          onBlur={() => {
            if (!isHexColor(value ?? DEFAULT_COLOR)) {
              setValue(themeColor);
            }
          }}
          onChange={(color: string) => setValue(color)}
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
