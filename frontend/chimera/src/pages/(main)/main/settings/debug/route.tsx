import { createFileRoute } from '@tanstack/react-router';
import { AnimatePresence } from 'motion/react';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsGroup,
  SettingsLabel,
} from '../_modules/settings-card';
import { SettingsTitle } from '../_modules/settings-title';
import AdvancedToolsSwitch from './_modules/advanced-tools-switch';
import BlockTaskViewer from './_modules/block-task-viewer';
import DebugProvider, { useDebugContext } from './_modules/debug-provider';
import KVStorage from './_modules/kv-storage';
import PathUtilsCard from './_modules/path-utils-card';

export const Route = createFileRoute('/(main)/main/settings/debug')({
  component: RouteComponent,
});

const DebugContent = () => {
  const { advancedTools } = useDebugContext();

  return (
    <>
      <SettingsTitle>{m.settings_label_debug()}</SettingsTitle>
      <div className="space-y-4 px-4 pb-4">
        <section data-slot="debug-path-settings-container">
          <SettingsLabel>{m.settings_label_debug()}</SettingsLabel>
          <PathUtilsCard />
        </section>

        <section data-slot="debug-advanced-settings-container">
          <SettingsLabel>Advanced Tools</SettingsLabel>
          <SettingsGroup>
            <SettingsCard>
              <SettingsCardContent>
                <AdvancedToolsSwitch />
              </SettingsCardContent>
            </SettingsCard>

            <AnimatePresence initial={false}>
              {advancedTools && (
                <>
                  <BlockTaskViewer />
                  <KVStorage />
                </>
              )}
            </AnimatePresence>
          </SettingsGroup>
        </section>
      </div>
    </>
  );
};

function RouteComponent() {
  return (
    <DebugProvider>
      <DebugContent />
    </DebugProvider>
  );
}
