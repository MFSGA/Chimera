/**
 * 配置文件页面父路由
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/route.tsx`
 *
 * 职责：
 * - 提供双栏布局：左侧侧栏（ProfilesNavigate）+ 右侧内容区（子路由）
 * - 提供 URL search 参数验证（支持从 URL 传递订阅链接）
 * - 通过 AnimatedOutletPreset 渲染子路由过渡动画
 *
 * 当前阶段（Profiles Step 2 — 侧栏导航迁移）：
 * - 添加 Sidebar 双栏布局，匹配 ref 的 profiles/route.tsx
 * - 左侧 SidebarContent 显示 ProfilesNavigate（按配置类型导航）
 * - 右侧使用 AppContentScrollArea + AnimatedOutletPreset
 * - 保留 validateSearch 验证 subscribeUrl/subscribeName/subscribeDesc
 *
 * 后续迁移计划：
 * - 添加 $type 子路由（按 ProfileType 过滤配置列表）
 * - 添加 inspect 子路由（配置内省）
 */

import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import ProfilesNavigate from './_modules/profiles-navigate';

/**
 * URL search 参数类型定义
 * 支持从 URL 传递订阅链接参数（用于 AddProfileDialog 预填）
 */
type ProfilePageSearch = {
  subscribeName?: string;
  subscribeUrl?: string;
  subscribeDesc?: string;
};

/**
 * 验证并转换可选的字符串参数
 */
const asOptionalString = (value: unknown) => {
  return typeof value === 'string' && value ? value : undefined;
};

/**
 * 验证并转换 URL 参数（必须是合法的 URL）
 */
const asValidUrl = (value: unknown) => {
  if (typeof value !== 'string' || !value) {
    return undefined;
  }

  try {
    new URL(value);
    return value;
  } catch {
    return undefined;
  }
};

export const Route = createFileRoute('/(main)/main/profiles')({
  // 验证 search 参数：支持 subscribeUrl / subscribeName / subscribeDesc
  validateSearch: (search): ProfilePageSearch => {
    const subscribeUrl = asValidUrl(search.subscribeUrl);

    return {
      subscribeUrl,
      subscribeName: asOptionalString(search.subscribeName),
      subscribeDesc: asOptionalString(search.subscribeDesc),
    };
  },
  component: RouteComponent,
});

/**
 * Profiles 页面布局组件
 *
 * 使用 Sidebar 组件提供双栏布局（匹配 ref）：
 * - 左侧：配置类型导航列表（SidebarContent + ProfilesNavigate）
 * - 右侧：配置列表内容区（AppContentScrollArea + AnimatedOutletPreset）
 */
function RouteComponent() {
  return (
    <Sidebar data-slot="profiles-container">
      {/* 左侧侧边栏：配置类型导航 */}
      <SidebarContent
        className="bg-surface-variant/10"
        data-slot="profiles-sidebar-scroll-area"
      >
        <ProfilesNavigate className="p-2" />
      </SidebarContent>

      {/* 
        右侧内容区域：配置列表
        使用 AppContentScrollArea 提供统一的滚动管理
      */}
      <AppContentScrollArea
        className={cn(
          'group/profiles-content flex-[3_1_auto]',
          // 为 AnimatedOutletPreset 过渡动画保留 overflow-clip
          'overflow-clip',
        )}
        data-slot="profiles-content-scroll-area"
      >
        <div
          className={cn(
            'container mx-auto w-full max-w-7xl',
            'flex min-h-full flex-col',
          )}
          data-slot="profiles-content"
        >
          <AnimatedOutletPreset className="flex flex-1 flex-col" />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  );
}
