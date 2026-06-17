/**
 * Dashboard 上下文 Provider
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/provider.tsx`
 *
 * 提供 Dashboard 页面的全局状态：
 * - openSheet: WidgetSheet（添加 widget 的抽屉）是否打开
 * - isEditing: 是否处于编辑模式（显示调整手柄和删除按钮）
 *
 * 在 route.tsx 中通过 DashboardProvider 注入整个 Dashboard 页面树。
 */

import { createContext, use, useState, type PropsWithChildren } from 'react';

const DashboardContext = createContext<{
  openSheet: boolean;
  setOpenSheet: (open: boolean) => void;
  isEditing: boolean;
  setIsEditing: (editing: boolean) => void;
} | null>(null);

/**
 * 获取 Dashboard 上下文的 Hook
 * 必须在 DashboardProvider 内部使用，否则抛出错误
 */
export const useDashboardContext = () => {
  const context = use(DashboardContext);

  if (!context) {
    throw new Error(
      'useDashboardContext must be used within a DashboardProvider',
    );
  }

  return context;
};

/**
 * Dashboard 上下文 Provider
 * 注入 openSheet/isEditing 状态到子组件树
 */
export function DashboardProvider({ children }: PropsWithChildren) {
  const [openSheet, setOpenSheet] = useState(false);
  const [isEditing, setIsEditing] = useState(false);

  return (
    <DashboardContext.Provider
      value={{
        openSheet,
        setOpenSheet,
        isEditing,
        setIsEditing,
      }}
    >
      {children}
    </DashboardContext.Provider>
  );
}
