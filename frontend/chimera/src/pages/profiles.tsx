import {
  RemoteProfileOptionsBuilder,
  useProfile,
  type RemoteProfile,
} from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { Public, TextSnippetOutlined, Update } from '@mui/icons-material';
import {
  Badge,
  Button,
  CircularProgress,
  Fab,
  Grid,
  IconButton,
} from '@mui/material';
import { createFileRoute, useLocation } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { AnimatePresence, motion } from 'framer-motion';
import { useAtom } from 'jotai';
import { useMemo, useState, useTransition } from 'react';
import { useTranslation } from 'react-i18next';
import { useWindowSize } from 'react-use';
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from '@/components/profiles/modules/store';
import NewProfileButton from '@/components/profiles/new-profile-button';
import {
  AddProfileContext,
  type AddProfileContextValue,
} from '@/components/profiles/profile-dialog';
import ProfileItem from '@/components/profiles/profile-item';
import { GlobalUpdatePendingContext } from '@/components/profiles/provider';
import { QuickImport } from '@/components/profiles/quick-import';
import RuntimeConfigDiffDialog from '@/components/profiles/runtime-config-diff-dialog';
import { ClashProfile, filterProfiles } from '@/components/profiles/utils';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

const parseSubscribeProfileContext = (search: string) => {
  const params = new URLSearchParams(search);
  const subscribeUrl = params.get('subscribeUrl');

  if (!subscribeUrl) {
    return null;
  }

  try {
    new URL(subscribeUrl);
  } catch {
    return null;
  }

  return {
    name: params.get('subscribeName'),
    desc: params.get('subscribeDesc'),
    url: subscribeUrl,
  } satisfies AddProfileContextValue;
};

function ProfilePage() {
  const { t } = useTranslation();

  const { query, update } = useProfile();
  const location = useLocation();

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);
  const addProfileCtxValue = useMemo(
    () => parseSubscribeProfileContext(window.location.search),
    [location],
  );
  // todo: optimize the components

  const onClickChains = (profile: ClashProfile) => {
    setGlobalChain(false);

    if (chainsSelected === profile.uid) {
      setChainsSelected(undefined);
    } else {
      setChainsSelected(profile.uid);
    }
  };

  const [globalChain, setGlobalChain] = useAtom(atomGlobalChainCurrent);

  const [chainsSelected, setChainsSelected] = useAtom(atomChainsSelected);

  const handleGlobalChainClick = () => {
    setChainsSelected(undefined);
    setGlobalChain(!globalChain);
  };

  const hasSide = globalChain || chainsSelected;

  const handleSideClose = () => {
    setChainsSelected(undefined);
    setGlobalChain(false);
  };

  const [runtimeConfigViewerOpen, setRuntimeConfigViewerOpen] = useState(false);
  const { width } = useWindowSize();
  const [globalUpdatePending, startGlobalUpdate] = useTransition();

  const handleGlobalProfileUpdate = useLockFn(async () => {
    startGlobalUpdate(async () => {
      const remoteProfiles =
        (profiles?.clash?.filter(
          (item) => item.type === 'remote',
        ) as RemoteProfile[]) || [];

      const updates: Array<Promise<void | null | undefined>> = [];

      for (const profile of remoteProfiles) {
        const option = {
          ...profile.option,
          update_interval: 0,
          user_agent: profile.option?.user_agent ?? null,
        } satisfies RemoteProfileOptionsBuilder;

        const result = await update.mutateAsync({
          uid: profile.uid,
          option,
        });
        updates.push(Promise.resolve(result));
      }

      try {
        await Promise.all(updates);
      } catch (error) {
        message(`failed to update profiles: \n${formatError(error)}`, {
          kind: 'error',
          title: t('Error'),
        });
      }
    });
  });

  return (
    <AddProfileContext.Provider value={addProfileCtxValue}>
      <SidePage
        title={t('Profiles')}
        flexReverse
        header={
          <div className="flex items-center gap-2">
            <RuntimeConfigDiffDialog
              open={runtimeConfigViewerOpen}
              onClose={() => setRuntimeConfigViewerOpen(false)}
            />
            <IconButton
              className="h-10 w-10"
              color="inherit"
              title={t('Runtime Config')}
              onClick={() => {
                setRuntimeConfigViewerOpen(true);
              }}
            >
              <TextSnippetOutlined />
            </IconButton>
            <Badge variant="dot">
              <Button
                size="small"
                variant={globalChain ? 'contained' : 'outlined'}
                onClick={handleGlobalChainClick}
                startIcon={<Public />}
              >
                {t('Global Proxy Chains')}
              </Button>
            </Badge>
          </div>
        }
      >
        <AnimatePresence initial={false} mode="sync">
          <GlobalUpdatePendingContext.Provider value={globalUpdatePending}>
            <div className="flex flex-col gap-4 p-6">
              <QuickImport defaultUrl={addProfileCtxValue?.url} />

              {profiles && (
                <Grid container spacing={2}>
                  {profiles.clash?.map((item) => (
                    <Grid
                      key={item.uid}
                      size={{
                        xs: 12,
                        sm: 12,
                        md: hasSide && width <= 1000 ? 12 : 6,
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
                          chainsSelected={chainsSelected === item.uid}
                        />
                      </motion.div>
                    </Grid>
                  ))}
                </Grid>
              )}
            </div>
          </GlobalUpdatePendingContext.Provider>
        </AnimatePresence>

        <div className="pointer-events-none fixed right-8 bottom-8 z-10 flex flex-col gap-3">
          <Fab
            className="pointer-events-auto"
            color="default"
            onClick={handleGlobalProfileUpdate}
            title={t('Update All Profiles')}
          >
            {globalUpdatePending ? <CircularProgress size={22} /> : <Update />}
          </Fab>
          <NewProfileButton className="pointer-events-auto" />
        </div>
      </SidePage>
    </AddProfileContext.Provider>
  );
}
