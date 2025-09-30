import { ReactNode, useEffect, useRef } from 'react';
import getSystem from '@/utils/get-system';

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  const str = getSystem();
  console.log(str);

  return (
    <div>
      <p>{str}</p>
      {children}
    </div>
  );
};
