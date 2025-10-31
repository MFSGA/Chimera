import Masonry from '@mui/lab/Masonry';
import SettingNyanpasuVersion from './setting-chimera-version';
import SettingSystemProxy from './setting-system-proxy';

export const SettingPage = () => {
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
      <SettingSystemProxy />

      <SettingNyanpasuVersion />
    </Masonry>
  );
};

export default SettingPage;
