import { createFileRoute } from '@tanstack/react-router';
import { check } from '@tauri-apps/plugin-updater';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/dashboard')({
  component: Dashboard,
});

async function getVersion() {
  const update = await check();
  console.log(update);
  if (update) {
    console.log('Update available:');

    // await update.downloadAndInstall();
    // await relaunch();
  }
}
function Dashboard() {
  const { t } = useTranslation();

  return (
    <div>
      <div
        className="cursor-pointer px-3 py-2 text-sm font-semibold text-blue-600 transition hover:text-blue-700"
        onClick={getVersion}
      >
        {t('action_get_version')}
      </div>
      <div>{t('page_dashboard')}</div>
    </div>
  );
}
