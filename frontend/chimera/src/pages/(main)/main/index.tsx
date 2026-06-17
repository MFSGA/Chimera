/**
 * (main) 布局的默认 landing page
 *
 * 迁移自 ref: `src/pages/(main)/main/index.tsx`
 *
 * 将用户重定向到 Dashboard 主页。
 *
 * 变迁说明：
 * - 旧版 (legacy) 重定向到 memorizedRoute（记录的上次访问路径）
 * - 新版 (main) 始终以 Dashboard 为入口，与 ref 行为一致
 * - 未来可改为记忆用户上次访问的页面
 */

import { createFileRoute, Navigate } from '@tanstack/react-router';

export const Route = createFileRoute('/(main)/main/')({
  component: RouteComponent,
});

function RouteComponent() {
  // 匹配 ref: 默认跳转到 Dashboard 页面
  return <Navigate to="/main/dashboard" />;
}
