import Paper from '@mui/material/Paper';
import { ReactNode, useEffect, useRef } from 'react';
import styles from './app-container.module.scss';
import { DrawerContent } from './drawer-content';

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
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
      <div>{children}</div>
    </Paper>
  );
};
