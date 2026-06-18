import {
  ClashCores,
  useClashConnections,
  useClashCores,
  useClashVersion,
  useSetting,
  type ClashCore_Serialize,
} from '@chimera/interface';
import { BaseCard, ExpandMore, LoadingButton } from '@chimera/ui';
import { Box, Button, List, ListItem } from '@mui/material';
import { useLockFn, useReactive } from 'ahooks';
import { motion } from 'framer-motion';
import { useMemo, useState } from 'react';
import { OS } from '@/consts';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { ClashCoreItem } from './modules/clash-core';

export const SettingClashCore = () => {
  const loading = useReactive({
    mask: false,
  });

  const [expand, setExpand] = useState(false);

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
    return clashVersion?.premium
      ? `${clashVersion.version} Premium`
      : clashVersion?.meta
        ? `${clashVersion.version} Meta`
        : clashVersion?.version || '-';
  }, [clashVersion]);

  const changeClashCore = useLockFn(async (core: ClashCore_Serialize) => {
    try {
      loading.mask = true;
      try {
        await deleteConnections.mutateAsync(undefined);
      } catch (e) {
        console.error(e);
      }

      await switchCore.mutateAsync(core);

      message('Successfully switched to the clash core: ' + ClashCores[core], {
        kind: 'info',
        title: 'Successful',
      });
    } catch (e) {
      message(
        'Failed to switch. You could see the details in the log: ' +
          `${e instanceof Error ? e.message : String(e)}`,
        {
          kind: 'error',
          title: 'Error',
        },
      );
    } finally {
      loading.mask = false;
    }
  });

  const handleRestart = async () => {
    try {
      await restartSidecar();

      message('Successfully restarted the core', {
        kind: 'info',
        title: 'Successful',
      });
    } catch (e) {
      message(
        'Failed to restart. You could see the details in the log' +
          formatError(e),
        {
          kind: 'error',
          title: 'Error',
        },
      );
    }
  };

  const handleCheckUpdates = async () => {
    try {
      await fetchRemote.mutateAsync();
    } catch (e) {
      message(
        'Failed to fetch. Please check your network connection' +
          '\n' +
          formatError(e),
        {
          kind: 'error',
          title: 'Error',
        },
      );
    }
  };

  return (
    <BaseCard
      label={m.settings_clash_core_manager_card_title()}
      loading={loading.mask}
      labelChildren={<span>{version}</span>}
    >
      <List disablePadding>
        {clashCores.data &&
          Object.entries(clashCores.data).map(([core, item]) => {
            const show = expand || core === currentCore;

            return (
              <motion.div
                key={item.name}
                animate={show ? 'open' : 'closed'}
                variants={{
                  open: {
                    height: 'auto',
                    opacity: 1,
                    scale: 1,
                  },
                  closed: {
                    height: 0,
                    opacity: 0,
                    scale: 0.7,
                  },
                }}
                transition={{
                  type: 'spring',
                  bounce: 0,
                  duration: 0.35,
                }}
              >
                <ClashCoreItem
                  data={item}
                  core={core as ClashCore_Serialize}
                  selected={core === currentCore}
                  onClick={() => changeClashCore(core as ClashCore_Serialize)}
                />
              </motion.div>
            );
          })}

        <ListItem
          sx={{
            pl: 0,
            pr: 0,
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <Box sx={{ display: 'flex', gap: 1 }}>
            <Button variant="outlined" onClick={handleRestart}>
              {m.settings_clash_core_manager_card_restart_sidecar()}
            </Button>

            {/** TODO: Support Linux when Manifest v2 released */}
            {OS !== 'linux' && (
              <LoadingButton
                variant="contained"
                loading={fetchRemote.isPending}
                onClick={handleCheckUpdates}
              >
                {m.settings_clash_core_manager_card_fetch_remote()}
              </LoadingButton>
            )}
          </Box>

          <ExpandMore expand={expand} onClick={() => setExpand(!expand)} />
        </ListItem>
      </List>
    </BaseCard>
  );
};

export default SettingClashCore;
