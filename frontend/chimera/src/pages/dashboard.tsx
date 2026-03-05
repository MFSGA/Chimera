import { commands, unwrapResult } from '@chimera/interface';
import { Button } from '@mui/material';
import Grid from '@mui/material/Grid';
import { createFileRoute } from '@tanstack/react-router';
import { relaunch } from '@tauri-apps/plugin-process';
import { check } from '@tauri-apps/plugin-updater';
import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import ServiceShortcuts from '@/components/dashboard/service-shortcuts';

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
      const res = await commands.greet('test');
      setGreetMessage(res);
    } catch (error) {
      console.error('Failed to call greet command', error);
      setGreetMessage('Failed to reach backend greet command.');
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-3">
        <div
          className="cursor-pointer px-3 py-2 text-sm font-semibold text-blue-600 transition hover:text-blue-700"
          onClick={getVersion}
        >
          {t('action_get_version')}
        </div>

        <div className="text-lg font-semibold">{t('page_dashboard')}</div>

        <div className="flex flex-wrap items-center gap-3">
          <Button variant="contained" color="primary" onClick={handleGreet}>
            send greet command
          </Button>
          <p className="text-sm text-neutral-600">{greetMessage}</p>
        </div>
      </div>

      <Grid container spacing={2}>
        <ServiceShortcuts />
      </Grid>
    </div>
  );
}
