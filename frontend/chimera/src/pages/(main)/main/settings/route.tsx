/**
 * 设置页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/settings/route.tsx`
 *
 * 职责：
 * - 提供双栏布局：左侧侧边栏导航 + 右侧内容区域
 * - 左侧 SidebarContent 显示 SettingsNavigate（设置子路由导航列表）
 * - 右侧 AppContentScrollArea 通过 AnimatedOutletPreset 渲染子路由
 * - 使用 ref 相同的 Sidebar 布局模式（与 Proxies 一致）
 */

import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import SettingsNavigate from './_modules/settings-navigate';

export const Route = createFileRoute('/(main)/main/settings')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <Sidebar data-slot="settings-container">
      {/* 左侧侧边栏：设置子路由导航列表 */}
      <SidebarContent
        className="bg-surface-variant/10 [&>div>div]:block!"
        data-slot="settings-sidebar-scroll-area"
      >
        <SettingsNavigate />
      </SidebarContent>

      {/* 
        右侧内容区域：设置子路由详情
        使用 AppContentScrollArea 统一滚动管理
        容器 max-w-7xl 匹配 ref 的最大宽度限制
      */}
      <AppContentScrollArea
        className={cn(
          'group/settings-content flex-[3_1_auto]',
          // 为 AnimatedOutletPreset 过渡动画保留 overflow-clip
          'overflow-clip',
        )}
        data-slot="settings-content-scroll-area"
      >
        <div
          className={cn('container mx-auto w-full max-w-7xl', 'min-h-full')}
          data-slot="settings-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  );
}
