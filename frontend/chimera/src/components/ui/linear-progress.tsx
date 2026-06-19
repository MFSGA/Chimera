/**
 * LinearProgress 线性进度条组件
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/components/ui/progress.tsx` 中的 LinearProgress
 * 用于显示订阅使用量进度
 */

import { cn } from '@chimera/ui';
import type { ComponentProps } from 'react';

export interface LinearProgressProps extends ComponentProps<'div'> {
  value?: number;
}

export function LinearProgress({
  value = 0,
  className,
  ...props
}: LinearProgressProps) {
  const clampedValue = Math.min(100, Math.max(0, value));

  return (
    <div
      className={cn(
        'bg-surface-variant h-1.5 w-full overflow-hidden rounded-full',
        className,
      )}
      data-slot="linear-progress"
      role="progressbar"
      aria-valuenow={clampedValue}
      aria-valuemin={0}
      aria-valuemax={100}
      {...props}
    >
      <div
        className="bg-primary h-full rounded-full transition-all duration-300 ease-out"
        data-slot="linear-progress-bar"
        style={{ width: `${clampedValue}%` }}
      />
    </div>
  );
}
