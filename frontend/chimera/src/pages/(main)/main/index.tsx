import { commands, useSetting } from '@chimera/interface';
import { Button, ListItem, ListItemText } from '@mui/material';
import { createFileRoute, Link } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useLockFn } from 'ahooks';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

export const Route = createFileRoute('/(main)/main/')({
  component: MainIndex,
});

const ExperimentalSwitch = () => {
  const { upsert } = useSetting('window_type');

  const handleClick = useLockFn(async () => {
    await upsert('legacy');
    await commands.createLegacyWindow();
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

function MainIndex() {
  const windowType = useSetting('window_type');

  const handleSwitchToLegacy = async () => {
    try {
      if (windowType.value !== 'legacy') {
        await windowType.upsert('legacy');
      }

      const result = await commands.createLegacyWindow();

      if (result.status !== 'ok') {
        throw new Error(result.error);
      }

      await getCurrentWebviewWindow().close();
    } catch (error) {
      message(`Failed to open legacy UI: ${formatError(error)}`, {
        kind: 'error',
        title: 'Error',
      });
    }
  };

  return (
    <main className="text-on-surface flex h-full flex-col gap-4 p-6">
      <section className="flex flex-col gap-2">
        <h1 className="text-2xl font-semibold">Main UI</h1>
        <p className="text-on-surface-variant max-w-2xl text-sm">
          This is the isolated new main layout shell. Legacy routes remain
          unchanged and can still be opened from their original paths.
        </p>
      </section>

      <div className="flex gap-2">
        <ExperimentalSwitch />
        {/*  <Button asChild>
          <Link to="/">Open Legacy Route</Link>
        </Button> */}
      </div>
    </main>
  );
}
