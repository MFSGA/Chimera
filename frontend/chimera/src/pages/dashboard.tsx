import { createFileRoute } from '@tanstack/react-router';
import { check } from '@tauri-apps/plugin-updater';

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
  // todo: use i18n
  // const { t } = useTranslation()

  return (
    <div>
      <div
        className="cursor-pointer px-3 py-2 text-sm font-semibold text-blue-600 transition hover:text-blue-700"
        onClick={getVersion}
      >
        get version
      </div>
      <div>dashboard</div>
    </div>
  );
}
