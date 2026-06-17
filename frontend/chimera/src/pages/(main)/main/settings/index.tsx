/**
 * 设置页面入口（默认子路由）
 *
 * 迁移自 ref: `src/pages/(main)/main/settings/index.tsx`
 *
 * 职责：
 * - 作为 `/main/settings` 的默认子路由（在 AnimatedOutletPreset 中渲染）
 * - 目前托管 legacy SettingPageComponent（全量设置页）
 * - 后续迁移计划：逐步将各设置模块拆分为独立子路由，此页面最终对齐 ref：
 *   - 桌面端：返回 null（导航已由父路由 Sidebar 提供）
 *   - 移动端：显示 SettingsNavigate（全屏导航）
 *
 * 适配说明：
 * - 移除了 AppContentScrollArea（父路由 route.tsx 已提供统一滚动管理）
 * - 头部 GitHub/Feedback 按钮已移除（功能将在各子路由中分别实现）
 * - 使用懒加载 SettingPageComponent 保持启动性能
 * - data-slot 属性标记便于自动化测试定位
 */

import { createFileRoute } from '@tanstack/react-router';
import { lazy, Suspense } from 'react';

// 延迟加载设置页组件（较大，按需加载）
const SettingPageComponent = lazy(
  () => import('@/components/setting/setting-page'),
);

export const Route = createFileRoute('/(main)/main/settings/')({
  component: RouteComponent,
});

function RouteComponent() {
  // 后续迁移：匹配 ref 模式
  // 桌面端 return null（父路由已提供侧栏导航）
  // 移动端 return <SettingsNavigate />（全屏导航）
  //
  // 当前阶段：子路由尚未迁移完毕，保持 legacy 页面可访问
  // 待 `/settings/system` `/settings/clash` 等子路由全部迁移后，
  // 切换至 ref 的 index 逻辑（桌面 null + 移动端 SettingsNavigate）

  return (
    <div className="min-h-full p-4" data-slot="settings-legacy-content">
      <Suspense
        fallback={
          <div className="text-on-surface-variant flex items-center justify-center p-8">
            Loading...
          </div>
        }
      >
        <SettingPageComponent />
      </Suspense>
    </div>
  );
}
