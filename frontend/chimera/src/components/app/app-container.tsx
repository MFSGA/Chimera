import { ReactNode, useEffect, useRef } from "react";

export const AppContainer = ({
  children,
  isDrawer,
}: {
  children?: ReactNode;
  isDrawer?: boolean;
}) => {
  return <div>{children}</div>;
};
