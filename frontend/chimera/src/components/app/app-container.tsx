import { cn, getSystem } from '@chimera/ui';
import Paper from '@mui/material/Paper';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { useAtomValue } from 'jotai';
import { ReactNode } from 'react';
import { atomIsDrawerOnlyIcon } from '@/store';
import { LayoutControl } from '../layout/layout-control';
import styles from './app-container.module.scss';
import { DrawerContent } from './drawer-content';

const appWindow = getCurrentWebviewWindow();

const OS = getSystem();

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  const onlyIcon = useAtomValue(atomIsDrawerOnlyIcon);

  return (
    <Paper
      square
      elevation={0}
      className={styles.layout}
      onPointerDown={(e) => {
        if ((e.target as HTMLElement)?.dataset?.windrag) {
          appWindow.startDragging();
        }
      }}
    >
      {!isDrawer && (
        <div className={cn(onlyIcon ? 'w-20' : 'w-60')}>
          <DrawerContent data-tauri-drag-region onlyIcon={onlyIcon} />
        </div>
      )}

      <div className={styles.container}>
        {OS === 'windows' && (
          <LayoutControl className="!z-top fixed top-2 right-4" />
        )}

        <div
          className={OS === 'macos' ? 'h-10' : 'h-9'}
          data-tauri-drag-region
        />

        {children}
      </div>
    </Paper>
  );
};
