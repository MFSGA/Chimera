import { FC, ReactNode, Suspense } from 'react';

interface BasePageProps {
  title?: ReactNode;
  children?: ReactNode;
}

export const BasePage: FC<BasePageProps> = ({ children }) => {
  return <Suspense>{children}</Suspense>;
};
