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

import { Button } from '@chimera/ui/button';
import { cn } from '@chimera/utils';
import AddRounded from '~icons/material-symbols/add-rounded';
import DoneRounded from '~icons/material-symbols/done-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useDashboardContext } from '@/components/widgets/provider';
import { WidgetConfigSaveStatus } from '@/components/widgets/widget-config-menu';
import * as m from '@/paraglide/messages';

export default function EditAction() {
  const {
    isEditing,
    setIsEditing,
    setOpenSheet,
    saveStatus,
    configLoading,
    configReadError,
  } = useDashboardContext();

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
            disabled={configLoading || saveStatus.state !== 'saved'}
            className="flex h-8 items-center gap-1 px-3 text-sm text-nowrap"
            data-slot="dashboard-edit-save"
            onClick={() => setIsEditing(false)}
          >
            <DoneRounded />
            <span>{m.common_save()}</span>
          </Button>

          {(configLoading ||
            configReadError ||
            saveStatus.state !== 'saved') && (
            <div className="bg-surface absolute bottom-full left-1/2 mb-2 min-w-48 -translate-x-1/2 rounded-full px-3 py-1.5">
              <WidgetConfigSaveStatus />
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
