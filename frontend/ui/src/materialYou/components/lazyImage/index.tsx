import { useState } from 'react';
import { cn } from '../../../utils';

export interface LazyImageProps extends React.ImgHTMLAttributes<HTMLImageElement> {
  loadingClassName?: string;
}
export function LazyImage({
  className,
  loadingClassName,
  onLoad,
  ...others
}: LazyImageProps) {
  const [loading, setLoading] = useState(true);

  return (
    <>
      <div
        className={cn(
          'inline-block animate-pulse bg-slate-200 ring-1 ring-slate-200 dark:bg-slate-700 dark:ring-slate-700',
          className,
          loadingClassName,
          loading ? 'inline-block' : 'hidden',
        )}
      />
      <img
        {...others}
        onLoad={(event) => {
          setLoading(false);
          onLoad?.(event);
        }}
        className={cn(className, loading ? 'hidden' : 'inline-block')}
      />
    </>
  );
}
