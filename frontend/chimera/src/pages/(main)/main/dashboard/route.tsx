/**
 * Dashboard 路由父布局
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/route.tsx`
 *
 * 职责：
 * - 作为 `/main/dashboard` 路由的容器
 * - 提供 DashboardProvider（编辑/添加 widget 状态管理）
 * - 匹配 ref 的 DashboardProvider 包裹模式
 *
 * 路由结构：
 * - `/main/dashboard`（此路由）：仅 Outlet，包裹 DashboardProvider
 * - `/main/dashboard/`（index 子路由）：实际页面内容在 index.tsx 中
 *
 * DashboardProvider 提供：
 * - openSheet / setOpenSheet：控制 WidgetSheet（添加 widget 的抽屉）的开关
 * - isEditing / setIsEditing：控制编辑模式的开关
 */

import { createFileRoute, Outlet } from '@tanstack/react-router';
import { DashboardProvider } from './_modules/provider';

/**
 * 注册 TanStack Router 文件路由
 * 路径: `/main/dashboard`
 * 匹配 ref: `createFileRoute('/(main)/main/dashboard')`
 */
export const Route = createFileRoute('/(main)/main/dashboard')({
  component: RouteComponent,
});

/**
 * Dashboard 路由组件
 *
 * 使用 DashboardProvider 包裹所有子路由，
 * 使 index.tsx 中的 WidgetRender、EditAction、WidgetSheet
 * 能够通过 useDashboardContext 访问编辑/添加状态。
 *
 * 匹配 ref: route.tsx 中的 DashboardProvider 包裹模式
 */
function RouteComponent() {
  return (
    <DashboardProvider>
      <Outlet />
    </DashboardProvider>
  );
}
