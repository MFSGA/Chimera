import { createFileRoute } from '@tanstack/react-router';
import { AnimatePresence } from 'motion/react';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card';
import AdvanceToolsSwitch from './_modules/advance-tools-switch';
import BlockTaskViewer from './_modules/block-task-viewer';
import ConfigurationStatus from './_modules/configuration-status';
import { useDebugContext } from './_modules/debug-provider';
import KVStorage from './_modules/kv-storage';
import PathUtilsCard from './_modules/path-utils-card';
import WindowDebug from './_modules/window-debug';

export const Route = createFileRoute('/(main)/main/settings/debug/')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_debug() } },
});

const PathUtilsSettings = () => {
  return (
    <div data-slot="debug-settings-container">
      <SettingsLabel>{m.settings_label_debug()}</SettingsLabel>

      <PathUtilsCard />
    </div>
  );
};

const AdvanceToolsSettings = () => {
  const { advanceTools } = useDebugContext();

  return (
    <div data-slot="debug-settings-container">
      <SettingsLabel>Advance Tools</SettingsLabel>

      <SettingsGroup>
        <SettingsCard>
          <SettingsCardContent>
            <AdvanceToolsSwitch />
          </SettingsCardContent>
        </SettingsCard>

        <AnimatePresence initial={false}>
          {advanceTools && (
            <>
              <WindowDebug />

              <BlockTaskViewer />

              <KVStorage />
            </>
          )}
        </AnimatePresence>
      </SettingsGroup>
    </div>
  );
};

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />

      <div className="space-y-4 px-4 pb-4">
        <PathUtilsSettings />

        <ConfigurationStatus />

        <AdvanceToolsSettings />
      </div>
    </>
  );
}
