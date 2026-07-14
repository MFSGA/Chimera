/**
 * Dashboard 编辑操作浮动栏
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/edit-action.tsx`
 *
 * 在编辑模式下显示在 Dashboard 底部的浮动操作栏：
 * - "添加 Widget" 按钮：打开 WidgetSheet
 * - "保存" 按钮：退出编辑模式
 *
 * 使用 motion 的 AnimatePresence 实现进出场动画。
 */

import { cn } from '@chimera/ui';
import AddRounded from '~icons/material-symbols/add-rounded';
import DoneRounded from '~icons/material-symbols/done-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { Button } from '@/components/ui/button';
import * as m from '@/paraglide/messages';
import { useDashboardContext } from './provider';

export default function EditAction() {
  const { isEditing, setIsEditing, setOpenSheet } = useDashboardContext();

  return (
    <AnimatePresence>
      {isEditing && (
        <motion.div
          className={cn(
            'absolute bottom-8 left-1/2 z-10 -translate-x-1/2',
            'flex h-14 min-w-0 items-center justify-between gap-6 px-3',
            'dark:border-surface-variant/50 border-surface/50 rounded-full border',
            'dark:bg-surface/30 bg-surface-variant/30 backdrop-blur-lg',
          )}
          data-slot="dashboard-edit-header"
          initial={{
            scale: 0.8,
            opacity: 0,
            y: 128,
          }}
          animate={{
            scale: 1,
            opacity: 1,
            y: 0,
          }}
          exit={{
            scale: 0.8,
            opacity: 0,
            y: 128,
          }}
          transition={{
            type: 'spring',
            bounce: 0,
            duration: 0.45,
          }}
        >
          {/* 添加 Widget 按钮 - 打开 WidgetSheet */}
          <Button
            variant="raised"
            className="flex h-8 items-center gap-1 px-3 text-sm text-nowrap"
            onClick={() => setOpenSheet(true)}
          >
            <AddRounded />
            <span>{m.dashboard_add_widget()}</span>
          </Button>

          {/* 保存按钮 - 退出编辑模式 */}
          <Button
            variant="flat"
            className="flex h-8 items-center gap-1 px-3 text-sm text-nowrap"
            onClick={() => setIsEditing(false)}
          >
            <DoneRounded />
            <span>{m.common_done()}</span>
          </Button>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
