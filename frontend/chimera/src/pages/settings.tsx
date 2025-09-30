import { createFileRoute } from '@tanstack/react-router';
import { useAtom } from 'jotai';
import { useTranslation } from 'react-i18next';
import { languageAtom } from '@/store';
import { languageOptions } from '@/utils/language';

export const Route = createFileRoute('/settings')({
  component: SettingPage,
});

function SettingPage() {
  const { t } = useTranslation();
  const [language, setLanguage] = useAtom(languageAtom);

  return (
    <div className="space-y-4 p-4">
      <h1 className="text-xl font-semibold">{t('page_settings')}</h1>

      <label className="flex max-w-xs flex-col gap-2 text-sm">
        <span className="font-medium">{t('settings_language_label')}</span>
        <select
          className="rounded-md border border-neutral-300 px-3 py-2"
          value={language}
          onChange={(event) => setLanguage(event.target.value)}
        >
          {Object.entries(languageOptions).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
        <span className="text-xs text-neutral-500">
          {t('settings_language_description')}
        </span>
      </label>
    </div>
  );
}
