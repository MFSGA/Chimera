import { useProfile } from '@chimera/interface';
import { Close } from '@mui/icons-material';
import { IconButton } from '@mui/material';
import { useAtomValue } from 'jotai';
import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { SideChain } from './modules/side-chain';
import { atomChainsSelected, atomGlobalChainCurrent } from './modules/store';

export interface ProfileSideProps {
  onClose: () => void;
}

export const ProfileSide = ({ onClose }: ProfileSideProps) => {
  const { t } = useTranslation();
  const { query } = useProfile();

  const isGlobalChainCurrent = useAtomValue(atomGlobalChainCurrent);
  const currentProfileUid = useAtomValue(atomChainsSelected);

  const currentProfile = useMemo(() => {
    return query.data?.items?.find((item) => item.uid === currentProfileUid);
  }, [currentProfileUid, query.data?.items]);

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-start justify-between p-4 pr-2">
        <div>
          <div className="text-xl font-bold">
            {isGlobalChainCurrent
              ? t('Global Proxy Chains')
              : t('Proxy Chains')}
          </div>

          <div className="truncate opacity-80">
            {isGlobalChainCurrent ? t('All Profiles') : currentProfile?.name}
          </div>
        </div>

        <IconButton onClick={onClose}>
          <Close />
        </IconButton>
      </div>

      <div className="min-h-0 flex-1">
        <SideChain />
      </div>
    </div>
  );
};

export default ProfileSide;
