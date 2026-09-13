import { useIsAppImage } from '@chimera/interface';
import { useMediaQuery } from '@mui/material';
import { useTheme } from '@mui/material/styles';
import { useAtomValue } from 'jotai';
import { Children, type ReactNode } from 'react';
import { atomIsDrawerOnlyIcon } from '@/store';
import SettingChimeraMisc from './setting-chimera-misc';
import SettingChimeraPath from './setting-chimera-path';
import SettingChimeraTasks from './setting-chimera-tasks';
import SettingChimerauUI from './setting-chimera-ui';
import SettingChimeraVersion from './setting-chimera-version';
import { SettingClashBase } from './setting-clash-base';
import SettingClashCore from './setting-clash-core';
import SettingClashExternal from './setting-clash-external';
import SettingClashField from './setting-clash-field';
import SettingClashPort from './setting-clash-port';
import SettingClashWeb from './setting-clash-web';
import SettingSystemBehavior from './setting-system-behavior';
import SettingSystemProxy from './setting-system-proxy';
import SettingSystemService from './setting-system-service';
import SettingSystemTools from './setting-system-tools';

const SettingColumns = ({
  children,
  twoColumns,
}: {
  children: ReactNode;
  twoColumns: boolean;
}) => {
  const items = Children.toArray(children);
  const columns = twoColumns
    ? [
        items.filter((_, index) => index % 2 === 0),
        items.filter((_, index) => index % 2 === 1),
      ]
    : [items];

  return (
    <div className="flex w-full items-start gap-6">
      {columns.map((column, index) => (
        <div
          className="flex min-w-0 flex-1 flex-col gap-6"
          key={index === 0 ? 'primary' : 'secondary'}
        >
          {column}
        </div>
      ))}
    </div>
  );
};

export const SettingPage = () => {
  const isAppImage = useIsAppImage();
  const isDrawerOnlyIcon = useAtomValue(atomIsDrawerOnlyIcon);
  const theme = useTheme();
  const isMdUp = useMediaQuery(theme.breakpoints.up('md'));
  const isWideLayout = useMediaQuery('(min-width:1001px)');
  const twoColumns = isMdUp && (isDrawerOnlyIcon || isWideLayout);

  return (
    <SettingColumns twoColumns={twoColumns}>
      <SettingSystemProxy />
      <SettingChimerauUI />
      <SettingClashBase />
      <SettingClashPort />
      <SettingClashExternal />
      <SettingClashWeb />
      <SettingClashField />
      <SettingClashCore />
      <SettingSystemBehavior />
      {!isAppImage.data && <SettingSystemService />}
      <SettingSystemTools />
      <SettingChimeraTasks />
      <SettingChimeraMisc />
      <SettingChimeraPath />
      <SettingChimeraVersion />
    </SettingColumns>
  );
};

export default SettingPage;
