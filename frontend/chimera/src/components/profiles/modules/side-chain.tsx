import { useProfile } from '@chimera/interface';
import { Button, Chip } from '@mui/material';
import { useAtomValue } from 'jotai';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import ContentDisplay from '@/components/base/content-display';
import { ProfileDialog } from '../profile-dialog';
import { filterProfiles } from '../utils';
import { atomChainsSelected, atomGlobalChainCurrent } from './store';

export const SideChain = () => {
  const { t } = useTranslation();
  const { query } = useProfile();
  const items = query.data?.items ?? [];

  const isGlobalChainCurrent = useAtomValue(atomGlobalChainCurrent);
  const currentProfileUid = useAtomValue(atomChainsSelected);

  const [dialogOpen, setDialogOpen] = useState(false);

  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);

  const currentProfile = useMemo(() => {
    return profiles.clash?.find((item) => item.uid === currentProfileUid);
  }, [currentProfileUid, profiles.clash]);

  const remoteCount = items.filter((item) => item.type === 'remote').length;
  const localCount = items.length - remoteCount;

  const handleOpenFile = () => {
    const view = (
      currentProfile as typeof currentProfile & {
        view?: () => Promise<null | undefined>;
      }
    )?.view;

    void view?.();
  };

  return (
    <>
      <div className="flex h-full flex-col gap-4 p-4">
        <div className="rounded-3xl border border-white/10 p-4">
          <div className="text-sm opacity-70">{t('Profile')}</div>
          <div className="mt-1 text-lg font-bold">
            {isGlobalChainCurrent
              ? t('All Profiles')
              : (currentProfile?.name ?? '-')}
          </div>

          {isGlobalChainCurrent ? (
            <div className="mt-4 flex flex-wrap gap-2">
              <Chip
                size="small"
                label={`${t('Profiles')}: ${profiles.clash?.length ?? 0}`}
              />
              <Chip size="small" label={`${t('Remote')}: ${remoteCount}`} />
              <Chip size="small" label={`${t('Local')}: ${localCount}`} />
            </div>
          ) : (
            <>
              <div className="mt-2 text-sm opacity-70">
                {currentProfile?.type === 'remote'
                  ? t('Remote Profile')
                  : t('Local Profile')}
              </div>

              {currentProfile?.desc && (
                <p className="mt-2 text-sm break-all opacity-80">
                  {currentProfile.desc}
                </p>
              )}

              {currentProfile?.type === 'remote' && currentProfile.url && (
                <p className="mt-2 line-clamp-3 text-xs break-all opacity-70">
                  {currentProfile.url}
                </p>
              )}

              <div className="mt-4 flex gap-2">
                <Button
                  size="small"
                  variant="contained"
                  onClick={() => setDialogOpen(true)}
                >
                  {t('Edit Profile')}
                </Button>

                <Button
                  size="small"
                  variant="outlined"
                  onClick={handleOpenFile}
                >
                  {t('Open File')}
                </Button>
              </div>
            </>
          )}
        </div>

        <div className="rounded-3xl border border-white/10 p-4">
          <div className="mb-2 text-sm opacity-70">{t('Chains')}</div>

          <ContentDisplay
            className="min-h-48"
            message={t('Chain profiles are not available in this build yet')}
          />
        </div>
      </div>

      <ProfileDialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        profile={currentProfile}
      />
    </>
  );
};

export default SideChain;
