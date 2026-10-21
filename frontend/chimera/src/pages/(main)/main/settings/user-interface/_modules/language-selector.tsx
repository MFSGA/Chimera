import ArrowForwardIosRounded from '~icons/material-symbols/arrow-forward-ios-rounded';
import { useRef } from 'react';
import { useLanguage } from '@/components/providers/language-provider';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import * as m from '@/paraglide/messages';
import type { Locale } from '@/paraglide/runtime';
import { languageOptions } from '@/utils/language';
import {
  ItemContainer,
  ItemLabel,
  ItemLabelDescription,
  ItemLabelText,
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

export default function LanguageSelector() {
  const { language, setLanguage } = useLanguage();
  const keyboardOpening = useRef(false);
  const currentLocale = (language || 'en') as Locale;

  return (
    <SettingsCard data-slot="language-selector-card">
      <DropdownMenu>
        <DropdownMenuTrigger
          asChild
          onKeyDown={(event) => {
            if (['Enter', ' ', 'ArrowDown'].includes(event.key)) {
              keyboardOpening.current = true;
            }
          }}
          onPointerDown={() => {
            keyboardOpening.current = false;
          }}
        >
          <SettingsCardContent data-slot="language-selector-trigger" asChild>
            <Button className="text-on-surface! h-auto w-full rounded-none px-5 text-left text-base">
              <ItemContainer>
                <ItemLabel>
                  <ItemLabelText>
                    {m.settings_user_interface_language_label()}
                  </ItemLabelText>
                  <ItemLabelDescription>
                    {languageOptions[currentLocale]}
                  </ItemLabelDescription>
                </ItemLabel>
                <ArrowForwardIosRounded />
              </ItemContainer>
            </Button>
          </SettingsCardContent>
        </DropdownMenuTrigger>

        <DropdownMenuContent
          align="end"
          sideOffset={-16}
          alignOffset={16}
          data-slot="language-selector-menu"
          onFocusCapture={(event) => {
            if (
              !keyboardOpening.current ||
              event.target !== event.currentTarget
            ) {
              return;
            }

            keyboardOpening.current = false;
            const checkedOption =
              event.currentTarget.querySelector<HTMLElement>(
                '[role="menuitemcheckbox"][aria-checked="true"]',
              );

            if (checkedOption) {
              // Keep Radix's container-focus event from selecting a different item.
              event.stopPropagation();
              checkedOption.focus();
            }
          }}
        >
          {Object.entries(languageOptions).map(([locale, label]) => (
            <DropdownMenuCheckboxItem
              checked={currentLocale === locale}
              key={locale}
              onSelect={() => setLanguage(locale as Locale)}
            >
              {label}
            </DropdownMenuCheckboxItem>
          ))}
        </DropdownMenuContent>
      </DropdownMenu>
    </SettingsCard>
  );
}
