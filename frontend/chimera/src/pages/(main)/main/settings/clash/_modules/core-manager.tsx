import {
  ClashCores,
  useClashConnections,
  useClashCores,
  useClashVersion,
  useSetting,
  type ClashCore_Serialize,
} from '@chimera/interface';
import ExpandMoreRounded from '~icons/material-symbols/expand-more-rounded';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import RestartAltRounded from '~icons/material-symbols/restart-alt-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useMemo, useState } from 'react';
import { ClashCoreItem } from '@/components/setting/modules/clash-core';
import {
  SettingsCard,
  SettingsCardContent,
  SettingsCardFooter,
} from '@/components/settings/settings-card';
import { Button } from '@/components/ui/button';
import { OS } from '@/consts';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export default function CoreManager() {
  const [expanded, setExpanded] = useState(false);
  const [switching, setSwitching] = useState(false);
  const { value: currentCore } = useSetting('clash_core');
  const {
    query: clashCores,
    upsert: switchCore,
    restartSidecar,
    fetchRemote,
  } = useClashCores();
  const { data: clashVersion } = useClashVersion();
  const { deleteConnections } = useClashConnections();

  const version = useMemo(() => {
    if (clashVersion?.premium) return `${clashVersion.version} Premium`;
    if (clashVersion?.meta) return `${clashVersion.version} Meta`;
    return clashVersion?.version || '-';
  }, [clashVersion]);

  const handleSwitchCore = async (core: ClashCore_Serialize) => {
    try {
      setSwitching(true);
      try {
        await deleteConnections.mutateAsync(undefined);
      } catch (error) {
        console.error(error);
      }
      await switchCore.mutateAsync(core);
      message(m.settings_clash_core_switch_success() + ClashCores[core], {
        kind: 'info',
        title: m.common_success(),
      });
    } catch (error) {
      message(`${m.settings_clash_core_switch_failed()}${formatError(error)}`, {
        kind: 'error',
        title: m.common_error(),
      });
    } finally {
      setSwitching(false);
    }
  };

  const handleRestart = async () => {
    try {
      await restartSidecar();
      message(m.settings_clash_core_restart_success(), {
        kind: 'info',
        title: m.common_success(),
      });
    } catch (error) {
      message(
        `${m.settings_clash_core_restart_failed()}${formatError(error)}`,
        {
          kind: 'error',
          title: m.common_error(),
        },
      );
    }
  };

  const handleFetchRemote = async () => {
    try {
      await fetchRemote.mutateAsync();
    } catch (error) {
      message(
        `${m.settings_clash_core_fetch_failed()}\n${formatError(error)}`,
        {
          kind: 'error',
          title: m.common_error(),
        },
      );
    }
  };

  return (
    <SettingsCard className="relative" data-slot="core-manager-card">
      <SettingsCardContent className="gap-2">
        <div className="flex items-center justify-between gap-3 px-1">
          <span className="text-on-surface-variant text-sm">{version}</span>
          {switching && <span className="text-sm">{m.common_loading()}</span>}
        </div>

        <AnimatePresence initial={false}>
          {clashCores.data &&
            Object.entries(clashCores.data).map(([core, item]) => {
              const visible = expanded || core === currentCore;
              if (!visible) return null;

              return (
                <motion.div
                  key={core}
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: 'auto', opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                >
                  <ClashCoreItem
                    data={item}
                    core={core as ClashCore_Serialize}
                    selected={core === currentCore}
                    onClick={() =>
                      void handleSwitchCore(core as ClashCore_Serialize)
                    }
                  />
                </motion.div>
              );
            })}
        </AnimatePresence>
      </SettingsCardContent>

      <SettingsCardFooter className="flex-wrap gap-2">
        <Button
          variant="flat"
          className="flex items-center gap-1"
          onClick={() => void handleRestart()}
        >
          <RestartAltRounded />
          {m.settings_clash_core_manager_card_restart_sidecar()}
        </Button>

        {OS !== 'linux' && (
          <Button
            variant="flat"
            className="flex items-center gap-1"
            loading={fetchRemote.isPending}
            onClick={() => void handleFetchRemote()}
          >
            <RefreshRounded />
            {m.settings_clash_core_manager_card_fetch_remote()}
          </Button>
        )}

        <div className="flex-1" />

        <Button
          icon
          variant="flat"
          aria-label={expanded ? 'Collapse' : 'Expand'}
          onClick={() => setExpanded((value) => !value)}
        >
          <ExpandMoreRounded
            className={
              expanded
                ? 'rotate-180 transition-transform'
                : 'transition-transform'
            }
          />
        </Button>
      </SettingsCardFooter>
    </SettingsCard>
  );
}
