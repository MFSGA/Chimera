import { ReactNode, useEffect, useRef } from 'react';
import { DrawerContent } from './drawer-content';

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  return (
    <div>
      {!isDrawer && (
        <div>
          <DrawerContent />
        </div>
      )}
      <div>{children}</div>
    </div>
  );
};
