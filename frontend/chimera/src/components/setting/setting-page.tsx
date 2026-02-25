import Masonry from '@mui/lab/Masonry';
import { useIsAppImage } from '@/hooks/use-consts';
import SettingChimerauUI from './setting-chimera-ui';
import SettingNyanpasuVersion from './setting-chimera-version';
import SettingClashCore from './setting-clash-core';
import SettingSystemProxy from './setting-system-proxy';
import SettingSystemService from './setting-system-service';

export const SettingPage = () => {
  const isAppImage = useIsAppImage();

  return (
    <Masonry
      className="w-full"
      columns={{
        xs: 1,
        sm: 1,
        md: 1,
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
      {/* 4 */}
      <SettingClashCore />
      {/* 5 */}
      {!isAppImage.data && <SettingSystemService />}
      {/* 2 */}
      <SettingNyanpasuVersion />
    </Masonry>
  );
};

export default SettingPage;
