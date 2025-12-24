import { alpha, getSystem } from '@chimera/ui';
import { Box } from '@mui/material';
import Paper from '@mui/material/Paper';
import { useSuspenseQuery } from '@tanstack/react-query';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ReactNode } from 'react';
import { LayoutControl } from '../layout/layout-control';
import styles from './app-container.module.scss';
import { DrawerContent } from './drawer-content';

// todo: add the css style file

const appWindow = getCurrentWebviewWindow();

const OS = getSystem();

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  const { data: isMaximized } = useSuspenseQuery({
    queryKey: ['isMaximized'],
    queryFn: () => appWindow.isMaximized(),
  });

  return (
    <Paper
      square
      elevation={0}
      className={styles.layout}
      /* todo: 
      onPointerDown={(e: any) => {
        if (e.target?.dataset?.windrag) {
          appWindow.startDragging();
        }
      }} */
      onContextMenu={(e) => {
        e.preventDefault();
      }}
    >
      {!isDrawer && (
        <div>
          <DrawerContent />
        </div>
      )}

      <div className={styles.container}>
        {OS === 'windows' && (
          <LayoutControl className="!z-top fixed top-2 right-4" />
        )}
        {/* TODO: add a framer motion animation to toggle the maximized state */}
        {OS === 'macos' && !isMaximized && (
          <Box
            className="z-top fixed top-1.5 left-3 h-7 w-[4.5rem] rounded-full"
            sx={(theme) => ({
              backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
            })}
          />
        )}

        <div
          className={OS === 'macos' ? 'h-[2.75rem]' : 'h-9'}
          data-tauri-drag-region
        />

        {children}
      </div>
    </Paper>
  );
};
