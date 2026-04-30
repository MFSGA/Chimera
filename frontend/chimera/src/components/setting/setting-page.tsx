import Masonry from '@mui/lab/Masonry';
import { useAtomValue } from 'jotai';
import { useWindowSize } from 'react-use';
import { useIsAppImage } from '@/hooks/use-consts';
import { atomIsDrawerOnlyIcon } from '@/store';
import SettingChimeraMisc from './setting-chimera-misc';
import SettingChimeraPath from './setting-chimera-path';
import SettingChimeraTasks from './setting-chimera-tasks';
import SettingChimerauUI from './setting-chimera-ui';
import SettingNyanpasuVersion from './setting-chimera-version';
import { SettingClashBase } from './setting-clash-base';
import SettingClashCore from './setting-clash-core';
import SettingClashExternal from './setting-clash-external';
import SettingClashField from './setting-clash-field';
import SettingClashPort from './setting-clash-port';
import SettingSystemBehavior from './setting-system-behavior';
import SettingSystemProxy from './setting-system-proxy';
import SettingSystemService from './setting-system-service';

export const SettingPage = () => {
  const isAppImage = useIsAppImage();
  const isDrawerOnlyIcon = useAtomValue(atomIsDrawerOnlyIcon);
  const { width } = useWindowSize();

  return (
    <Masonry
      className="w-full"
      columns={{
        xs: 1,
        sm: 1,
        md: isDrawerOnlyIcon ? 2 : width > 1000 ? 2 : 1,
        lg: 2,
        xl: 2,
      }}
      spacing={3}
      sequential
    >
      {/* 1 */}
      <SettingSystemProxy />
      {/* 3 */}
      <SettingChimerauUI />
      {/* 6 */}
      <SettingClashBase />
      <SettingClashPort />
      <SettingClashExternal />
      <SettingClashField />
      {/* 4 */}
      <SettingClashCore />
      <SettingSystemBehavior />
      {/* 5 */}
      {!isAppImage.data && <SettingSystemService />}
      <SettingChimeraTasks />
      <SettingChimeraMisc />
      <SettingChimeraPath />
      {/* 2 */}
      <SettingNyanpasuVersion />
    </Masonry>
  );
};

export default SettingPage;
