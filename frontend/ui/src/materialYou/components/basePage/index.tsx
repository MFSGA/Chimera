import { FC, ReactNode, Suspense } from 'react';
import { BaseErrorBoundary } from './baseErrorBoundary';
import './style.scss';
import Header from './header';

interface BasePageProps {
  title?: ReactNode;
  header?: ReactNode;
  children?: ReactNode;
}

export const BasePage: FC<BasePageProps> = ({ title, header, children }) => {
  return (
    <BaseErrorBoundary>
      <div className="MDYBasePage" data-tauri-drag-region>
        <Header title={title} header={header} />

        <Suspense>{children}</Suspense>
      </div>
    </BaseErrorBoundary>
  );
};
