import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/settings')({
  component: SettingPage,
});

function SettingPage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>settings</div>;
}
