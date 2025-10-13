import { FC, ReactNode } from 'react';
import { BaseErrorBoundary } from '../basePage/baseErrorBoundary';

interface Props {
  children?: ReactNode;
}
// todo
export const SidePage: FC<Props> = ({ children }) => {
  return (
    <BaseErrorBoundary>
      <div data-tauri-drag-region>{children}</div>
    </BaseErrorBoundary>
  );
};
