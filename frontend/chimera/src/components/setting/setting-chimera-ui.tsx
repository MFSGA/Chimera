import { useSetting } from '@chimera/interface';
import { BaseCard, Expand, MenuItem, SwitchItem } from '@chimera/ui';
import Done from '@mui/icons-material/Done';
import { Button, List, ListItem, ListItemText } from '@mui/material';
import { invoke } from '@tauri-apps/api/core';
import { useMemoizedFn } from 'ahooks';
import { useAtom } from 'jotai';
import { MuiColorInput } from 'mui-color-input';
import { useEffect, useState, useTransition } from 'react';
import { useTranslation } from 'react-i18next';
import { isHexColor } from 'validator';
import { atomIsDrawerOnlyIcon } from '@/store';
import { formatError } from '@/utils';
import { languageOptions } from '@/utils/language';
import { message } from '@/utils/notification';
import { DEFAULT_COLOR } from '../layout/use-custom-theme';

const commonSx = {
  width: 128,
};

const LanguageSwitch = () => {
  const { t } = useTranslation();

  const language = useSetting('language');

  return (
    <MenuItem
      label={t('Language')}
      selectSx={commonSx}
      options={languageOptions}
      selected={language.value || 'en'}
      onSelected={(value) => language.upsert(value as string)}
    />
  );
};

const ThemeSwitch = () => {
  const { t } = useTranslation();

  const themeOptions = {
    dark: t('theme.dark'),
    light: t('theme.light'),
    system: t('theme.system'),
  };

  const themeMode = useSetting('theme_mode');

  return (
    <MenuItem
      label={t('Theme Mode')}
      selectSx={commonSx}
      options={themeOptions}
      selected={themeMode.value || 'system'}
      onSelected={(value) => themeMode.upsert(value as string)}
    />
  );
};

const ThemeColor = () => {
  const { t } = useTranslation();

  const theme = useSetting('theme_color');

  const [value, setValue] = useState(theme.value ?? DEFAULT_COLOR);

  useEffect(() => {
    setValue(theme.value ?? DEFAULT_COLOR);
  }, [theme.value]);

  return (
    <>
      <ListItem sx={{ pl: 0, pr: 0 }}>
        <ListItemText primary={t('Theme Setting')} />

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
            {t('Apply')}
          </Button>
        </div>
      </Expand>
    </>
  );
};

export const SettingChimerauUI = () => {
  const { t } = useTranslation();

  const [onlyIcon, setOnlyIcon] = useAtom(atomIsDrawerOnlyIcon);
  const legacyMode = useSetting('use_legacy_ui');
  const [switchingLegacy, startSwitchingLegacy] = useTransition();

  const handleOpenLegacyWindow = useMemoizedFn(() => {
    startSwitchingLegacy(async () => {
      try {
        if (!legacyMode.value) {
          await legacyMode.upsert(true);
        }

        await invoke('create_legacy_window');
      } catch (error) {
        await message(
          `${t('Failed to open legacy window')}: ${formatError(error)}`,
          {
            kind: 'error',
            title: t('Error'),
          },
        );
      }
    });
  });

  return (
    <BaseCard label={t('User Interface')}>
      <List disablePadding>
        <LanguageSwitch />

        <ThemeSwitch />

        <ThemeColor />

        <SwitchItem
          label={t('Icon Navigation Bar')}
          checked={onlyIcon}
          onChange={() => setOnlyIcon(!onlyIcon)}
        />

        <ListItem sx={{ pl: 0, pr: 0 }}>
          <ListItemText
            primary={
              legacyMode.value
                ? t('Open Legacy Window')
                : t('Switch to Legacy UI')
            }
            secondary={
              legacyMode.value
                ? t('Legacy UI is already the preferred window mode')
                : t('Set Chimera to use the legacy window mode and open it')
            }
          />

          <Button
            variant="contained"
            onClick={handleOpenLegacyWindow}
            loading={switchingLegacy}
            disabled={switchingLegacy}
          >
            {legacyMode.value ? t('Open') : t('Switch')}
          </Button>
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingChimerauUI;
