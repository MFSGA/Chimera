import { useProfile } from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { Badge, Box, Button, Grid, IconButton } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { AnimatePresence, motion } from 'framer-motion';
import { useAtom } from 'jotai';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from '@/components/profiles/modules/store';
import ProfileItem from '@/components/profiles/profile-item';
import { QuickImport } from '@/components/profiles/quick-import';
import { ClashProfile, filterProfiles } from '@/components/profiles/utils';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  const { t } = useTranslation();

  const { query } = useProfile();

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);
  // todo: optimize the components

  const onClickChains = (profile: ClashProfile) => {
    console.log(profile);
  };

  const [chainsSelected, setChainsSelected] = useAtom(atomChainsSelected);

  const [globalChain, setGlobalChain] = useAtom(atomGlobalChainCurrent);

  const hasSide = globalChain || chainsSelected;

  const handleSideClose = () => {
    setChainsSelected(undefined);
    setGlobalChain(false);
  };

  const [runtimeConfigViewerOpen, setRuntimeConfigViewerOpen] = useState(false);

  return (
    <SidePage
      title={t('Profiles')}
      flexReverse
      header={
        <div className="flex items-center gap-2">
          {/* <RuntimeConfigDiffDialog
            open={runtimeConfigViewerOpen}
            onClose={() => setRuntimeConfigViewerOpen(false)}
          /> */}
          <IconButton
            className="h-10 w-10"
            color="inherit"
            title={t('Runtime Config')}
            onClick={() => {
              setRuntimeConfigViewerOpen(true);
            }}
          ></IconButton>
          <Badge
            variant="dot"
            // color={
            //   maxLogLevelTriggered.global === 'error'
            //     ? 'error'
            //     : maxLogLevelTriggered.global === 'warn'
            //       ? 'warning'
            //       : 'primary'
            // }
            // invisible={!maxLogLevelTriggered.global}
          >
            {/* <Button
              size="small"
              variant={globalChain ? 'contained' : 'outlined'}
              onClick={handleGlobalChainClick}
              startIcon={<Public />}
            >
              {t('Global Proxy Chains')}
            </Button> */}
          </Badge>
        </div>
      }
    >
      <AnimatePresence initial={false} mode="sync">
        <div className="flex flex-col gap-4 p-6">
          <QuickImport />

          {profiles && (
            <Grid container spacing={2}>
              {profiles.clash?.map((item) => (
                <Grid
                  key={item.uid}
                  size={{
                    xs: 12,
                    sm: 12,
                    // md: hasSide && width <= 1000 ? 12 : 6,
                    lg: 4,
                    xl: 3,
                  }}
                >
                  <motion.div
                    key={item.uid}
                    layoutId={`profile-${item.uid}`}
                    layout="position"
                    initial={false}
                  >
                    <ProfileItem
                      item={item}
                      onClickChains={onClickChains}
                      selected={query.data?.current?.includes(item.uid)}
                      // maxLogLevelTriggered={maxLogLevelTriggered}
                      chainsSelected={chainsSelected === item.uid}
                    />
                  </motion.div>
                </Grid>
              ))}
            </Grid>
          )}
        </div>
      </AnimatePresence>
    </SidePage>
  );
}
