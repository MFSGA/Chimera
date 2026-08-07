import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { Button } from '@/components/ui/button';
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardHeader,
} from '../../_modules/settings-card';

const currentWindow = getCurrentWebviewWindow();

export default function WindowDebug() {
  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Window Debug Utils</SettingsCardHeader>

        <SettingsCardContent>
          <div className="flex items-center gap-1 select-text">
            <span>Current Window Label:</span>
            <span className="font-mono font-bold">{currentWindow.label}</span>
          </div>

          <div className="flex items-center gap-2">
            <Button variant="flat" disabled>
              Create Test Editor Window
            </Button>
            <Button variant="flat" disabled>
              Create Persistent Tray Menu Window
            </Button>
          </div>
        </SettingsCardContent>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  );
}
