import { commands, unwrapResult } from '@chimera/interface';
import { Button } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/dashboard')({
  component: Dashboard,
});

async function getVersion() {
  const update = await check();
  console.log(update);
  if (update) {
    console.log(
      `found update ${update.version} from ${update.date} with notes ${update.body}`,
    );
    let downloaded = 0;
    let contentLength = 0;
    console.log('Update available:');

    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          contentLength = event.data.contentLength ?? 0;
          console.log(`started downloading ${event.data.contentLength} bytes`);
          break;
        case 'Progress':
          downloaded += event.data.chunkLength;
          console.log(`downloaded ${downloaded} from ${contentLength}`);
          break;
        case 'Finished':
          console.log('download finished');
          break;
      }
    });

    console.log('update installed');
    await relaunch();
  }
}
function Dashboard() {
  const { t } = useTranslation();
  const [greetMessage, setGreetMessage] = useState<string | null>(null);

  const handleGreet = async () => {
    try {
      const res = await commands.getGreet('test');
      setGreetMessage(unwrapResult(res) ?? 'errored msg');
    } catch (error) {
      console.error('Failed to call greet command', error);
      setGreetMessage('Failed to reach backend greet command.');
    }
  };

  return (
    <div>
      <div
        className="cursor-pointer px-3 py-2 text-sm font-semibold text-blue-600 transition hover:text-blue-700"
        onClick={getVersion}
      >
        {t('action_get_version')}
      </div>
      <div>{t('page_dashboard')}</div>
      <Button variant="contained" color="primary" onClick={handleGreet}>
        send greet command
      </Button>
      <p>{greetMessage}</p>
    </div>
  );
}
