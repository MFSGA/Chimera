import {
  RemoteProfileOptionsBuilder,
  useProfile,
  type RemoteProfile,
} from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import {
  ErrorOutline,
  Public,
  TextSnippetOutlined,
  Update,
} from '@mui/icons-material';
import {
  Badge,
  Button,
  CircularProgress,
  Fab,
  Grid,
  IconButton,
} from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { AnimatePresence, motion } from 'framer-motion';
import { useAtom } from 'jotai';
import { useMemo, useState, useTransition } from 'react';
import { useTranslation } from 'react-i18next';
import { useWindowSize } from 'react-use';
import ContentDisplay from '@/components/base/content-display';
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
import ProfileSide from '@/components/profiles/profile-side';
import { GlobalUpdatePendingContext } from '@/components/profiles/provider';
import { QuickImport } from '@/components/profiles/quick-import';
import RuntimeConfigDiffDialog from '@/components/profiles/runtime-config-diff-dialog';
import { ClashProfile, filterProfiles } from '@/components/profiles/utils';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const Route = createFileRoute('/profiles')({
  validateSearch: (search): ProfilePageSearch => {
    const subscribeUrl = asValidUrl(search.subscribeUrl);

    return {
      subscribeUrl,
      subscribeName: asOptionalString(search.subscribeName),
      subscribeDesc: asOptionalString(search.subscribeDesc),
    };
  },
  component: ProfilePage,
});

type ProfilePageSearch = {
  subscribeName?: string;
  subscribeUrl?: string;
  subscribeDesc?: string;
};

const asOptionalString = (value: unknown) => {
  return typeof value === 'string' && value ? value : undefined;
};

const asValidUrl = (value: unknown) => {
  if (typeof value !== 'string' || !value) {
    return undefined;
  }

  try {
    new URL(value);
    return value;
  } catch {
    return undefined;
  }
};

function ProfilePage() {
  const { t } = useTranslation();

  const { query, update } = useProfile();
  const search = Route.useSearch();

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);
  const profileItems = profiles?.clash ?? [];
  const addProfileCtxValue = useMemo(
    () =>
      search.subscribeUrl
        ? ({
            name: search.subscribeName ?? null,
            desc: search.subscribeDesc ?? null,
            url: search.subscribeUrl,
          } satisfies AddProfileContextValue)
        : null,
    [search],
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

  const renderProfilesContent = () => {
    if (query.isLoading) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex flex-col items-center gap-4 rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <CircularProgress size={28} />
              <div className="text-sm opacity-80">{t('Loading profiles')}</div>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    if (query.isError) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex w-full max-w-md flex-col items-center rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <ErrorOutline className="!mb-4 !size-14 text-red-400 dark:text-red-300" />
              <div className="text-base font-semibold">
                {t('Failed to load profiles')}
              </div>
              <div className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
                {formatError(query.error)}
              </div>
              <Button
                className="!mt-5"
                variant="contained"
                onClick={() => query.refetch()}
              >
                {t('Refresh')}
              </Button>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    if (!profileItems.length) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex w-full max-w-md flex-col items-center rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <Public className="!mb-4 !size-14 text-zinc-400 dark:text-zinc-500" />
              <div className="text-base font-semibold">{t('No Profiles')}</div>
              <div className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
                {t('Import a subscription URL to get started')}
              </div>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    return (
      <Grid container spacing={2}>
        {profileItems.map((item) => (
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
    );
  };

  return (
    <AddProfileContext.Provider value={addProfileCtxValue}>
      <SidePage
        title={t('Profiles')}
        flexReverse
        side={hasSide && <ProfileSide onClose={handleSideClose} />}
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
              <QuickImport />
              {renderProfilesContent()}
            </div>
          </GlobalUpdatePendingContext.Provider>
        </AnimatePresence>

        <div className="pointer-events-none fixed right-8 bottom-8 z-10 flex flex-col gap-3">
          <Fab
            className="pointer-events-auto"
            color="default"
            onClick={handleGlobalProfileUpdate}
            title={t('Update All Profiles')}
            disabled={
              globalUpdatePending ||
              query.isLoading ||
              query.isError ||
              !profileItems.length
            }
          >
            {globalUpdatePending ? <CircularProgress size={22} /> : <Update />}
          </Fab>
          <NewProfileButton className="pointer-events-auto" />
        </div>
      </SidePage>
    </AddProfileContext.Provider>
  );
}
