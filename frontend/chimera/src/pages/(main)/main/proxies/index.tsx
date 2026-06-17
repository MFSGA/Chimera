/**
 * 代理页面首页（移动端代理组列表）
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/index.tsx`
 *
 * 职责：
 * - 在移动端显示代理组导航列表
 * - 桌面端返回 null（组列表已在侧边栏中显示）
 *
 * 路由逻辑：
 * - 桌面端（!isMobile）：挂载此路由时返回 null，由 route.tsx 的 Sidebar 负责渲染组列表
 * - 移动端（isMobile）：显示 ProxiesNavigate 作为独立页面
 */

import { AppContentScrollArea } from '@/components/ui/scroll-area';
import useIsMobile from '@/hooks/use-is-moblie';
import { createFileRoute } from '@tanstack/react-router';
import ProxiesNavigate from './_modules/proxies-navigate';

export const Route = createFileRoute('/(main)/main/proxies/')({
  component: RouteComponent,
});

function RouteComponent() {
  const isMobile = useIsMobile();

  // 桌面端：侧边栏已在 route.tsx 中显示，这里不重复渲染
  if (!isMobile) {
    return null;
  }

  // 移动端：显示代理组列表作为独立页面
  return (
    <AppContentScrollArea
      className="bg-surface-variant/10"
      data-slot="proxies-sidebar-scroll-area"
    >
      <ProxiesNavigate />
    </AppContentScrollArea>
  );
}
