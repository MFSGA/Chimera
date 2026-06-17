/**
 * Dashboard Widget 容器项
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/widget-item.tsx`
 *
 * 包装 DndGridItem，添加编辑模式下的关闭按钮。
 * 关闭按钮通过 AnimatePresence 实现显隐动画。
 *
 * 在编辑模式（!disabled && !sourceOnly）时显示关闭按钮，
 * 点击后通过 onCloseClick 回调通知父组件删除该 widget。
 */

import { cn } from '@chimera/ui';
import CloseRounded from '~icons/material-symbols/close-rounded';
import { AnimatePresence, motion } from 'framer-motion';
import { Button } from '@/components/ui/button';
import { DndGridItem, type DndGridItemProps } from '@/components/ui/dnd-grid';
import { useDndGridContext } from '@/components/ui/dnd-grid/context';
import type { WidgetComponentProps } from './consts';

export type WidgetItemProps = DndGridItemProps<string> & WidgetComponentProps;

/**
 * WidgetItem — 可拖拽、可关闭的 widget 容器
 *
 * 用法：
 * ```tsx
 * <WidgetItem id={widgetId} minW={3} minH={2} onCloseClick={handleRemove}>
 *   <div>widget content</div>
 * </WidgetItem>
 * ```
 */
export default function WidgetItem({
  children,
  className,
  onCloseClick,
  ...props
}: WidgetItemProps) {
  const { disabled, sourceOnly } = useDndGridContext();

  return (
    <DndGridItem {...props} className={cn('relative', className)}>
      {children}

      {/* 编辑模式下显示关闭按钮 */}
      <AnimatePresence>
        {!disabled && !sourceOnly && (
          <Button
            variant="raised"
            className={cn(
              'absolute -top-1 -right-1 z-10 size-8',
              'border-outline/30 border',
            )}
            icon
            onClick={() => onCloseClick?.(props.id)}
            asChild
          >
            <motion.button
              initial={{
                scale: 0.85,
                opacity: 0,
              }}
              animate={{
                scale: 1,
                opacity: 1,
              }}
              exit={{
                scale: 0.85,
                opacity: 0,
              }}
              transition={{
                type: 'tween',
                duration: 0.1,
                ease: 'easeOut',
              }}
            >
              <CloseRounded className="size-4" />
            </motion.button>
          </Button>
        )}
      </AnimatePresence>
    </DndGridItem>
  );
}
