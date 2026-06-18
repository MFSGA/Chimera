import { commands, useSetting } from '@chimera/interface';
import { BaseCard, Expand, MenuItem, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useLockFn } from 'ahooks';
import { useAtom } from 'jotai';
import { MuiColorInput } from 'mui-color-input';
import { useEffect, useState } from 'react';
import { isHexColor } from 'validator';
import { useLanguage } from '@/components/providers/language-provider';
import * as m from '@/paraglide/messages';
import type { Locale } from '@/paraglide/runtime';
import { atomIsDrawerOnlyIcon } from '@/store';
import { languageOptions } from '@/utils/language';
import { DEFAULT_COLOR } from '../layout/use-custom-theme';

const currentWindow = getCurrentWebviewWindow();

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

  const themeMode = useSetting('theme_mode');

  return (
    <MenuItem
      label={m.settings_user_interface_theme_mode_label()}
      selectSx={commonSx}
      options={themeOptions}
      selected={themeMode.value || 'system'}
      onSelected={(value) => themeMode.upsert(value as string)}
    />
  );
};

const ThemeColor = () => {
  const theme = useSetting('theme_color');

  const [value, setValue] = useState(theme.value ?? DEFAULT_COLOR);

  useEffect(() => {
    setValue(theme.value ?? DEFAULT_COLOR);
  }, [theme.value]);

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
              setValue(theme.value ?? DEFAULT_COLOR);
            }
          }}
          onChange={(color: string) => setValue(color)}
        />
      </ListItem>

      <Expand open={(theme.value || DEFAULT_COLOR) !== value}>
        <div className="flex justify-end">
          <Button
            variant="contained"
            startIcon={<Done />}
            onClick={() => {
              if (isHexColor(value)) {
                theme.upsert(value);
              } else {
                // 如果输入的不是有效的十六进制颜色，则恢复为之前的值
                setValue(theme.value ?? DEFAULT_COLOR);
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
  const { upsert } = useSetting('window_type');

  const handleClick = useLockFn(async () => {
    await upsert('main');
    await commands.createMainWindow();
    await currentWindow.close();
  });

  return (
    <ListItem sx={{ pl: 0, pr: 0 }}>
      <ListItemText primary="Switch to Experimental UI" />

      <Button variant="contained" onClick={handleClick}>
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
