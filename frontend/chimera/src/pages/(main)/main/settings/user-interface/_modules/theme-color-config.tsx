import { cn } from '@chimera/ui';
import {
  DEFAULT_COLOR,
  useExperimentalThemeContext,
} from '@/components/providers/theme-provider';
import * as m from '@/paraglide/messages';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

const PRESET_COLORS = [
  DEFAULT_COLOR,
  '#9e1e67',
  '#3d009e',
  '#00089e',
  '#066b9e',
  '#9e5a00',
];

export default function ThemeColorConfig() {
  const { themeColor, setThemeColor } = useExperimentalThemeContext();

  return (
    <SettingsCard data-slot="theme-color-config-card">
      <SettingsCardContent>
        <ItemContainer>
          <ItemLabel>
            <ItemLabelText>
              {m.settings_user_interface_theme_color_label()}
            </ItemLabelText>
            <ItemLabelDescription className="flex items-center gap-1.5">
              <span
                className="inline-block size-3 rounded-full"
                style={{ backgroundColor: themeColor }}
              />
              <span>{themeColor}</span>
            </ItemLabelDescription>
          </ItemLabel>

          <div className="flex items-center gap-2">
            <div className="hidden items-center gap-1.5 sm:flex">
              {PRESET_COLORS.map((color) => (
                <button
                  type="button"
                  aria-label={color}
                  key={color}
                  className={cn(
                    'size-7 rounded-full border-2 transition-transform hover:scale-110',
                    themeColor === color
                      ? 'border-primary'
                      : 'border-transparent',
                  )}
                  style={{ backgroundColor: color }}
                  onClick={() => void setThemeColor(color)}
                />
              ))}
            </div>

            <input
              type="color"
              aria-label={m.settings_user_interface_theme_color_label()}
              className="border-outline-variant h-10 w-12 cursor-pointer rounded-xl border bg-transparent p-1"
              value={themeColor}
              onChange={(event) => void setThemeColor(event.target.value)}
            />
          </div>
        </ItemContainer>
      </SettingsCardContent>
    </SettingsCard>
  );
}
