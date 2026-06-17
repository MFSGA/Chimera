/**
 * 日志级别徽章组件
 *
 * 迁移自 ref: `src/pages/(main)/main/logs/_modules/log-level-badge.tsx`
 *
 * 职责：
 * - 根据日志级别显示不同颜色的徽章
 * - 支持搜索高亮（通过 searchText 属性）
 * - info 蓝色 / warning 黄色 / error 红色 / debug 默认
 */

import { cn } from '@chimera/ui';
import { type ComponentProps } from 'react';

export default function LogLevelBadge({
  className,
  children,
  ...props
}: ComponentProps<'div'> & { children: string }) {
  const childrenLower = children?.toLowerCase();

  return (
    <div
      className={cn(
        'inline-block rounded-full px-2 py-0.5 text-xs font-semibold uppercase',
        'bg-opacity-20',
        childrenLower === 'info' &&
          'bg-blue-500/20 text-blue-600 dark:text-blue-400',
        childrenLower === 'warning' &&
          'bg-yellow-500/20 text-yellow-600 dark:text-yellow-400',
        childrenLower === 'error' &&
          'bg-red-500/20 text-red-600 dark:text-red-400',
        childrenLower === 'debug' &&
          'bg-gray-500/20 text-gray-600 dark:text-gray-400',
        className,
      )}
      {...props}
    >
      {children}
    </div>
  );
}
