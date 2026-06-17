/**
 * Profiles Inspect 页面（占位）
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/inspect/route.tsx`
 *
 * 职责：
 * - 作为 `/main/profiles/inspect` 路由
 * - 用于内部检查和分析配置文件的结构、规则、代理等信息
 *
 * 当前阶段：占位实现
 * - 匹配 ref 的占位内容
 * - 后续可添加配置内省功能（YAML/JS/Lua 解析与可视化）
 */

import { createFileRoute } from '@tanstack/react-router';
import DescriptionOutlined from '@mui/icons-material/DescriptionOutlined';

export const Route = createFileRoute('/(main)/main/profiles/inspect')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <div
      className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4"
      data-slot="profiles-inspect-container"
    >
      <DescriptionOutlined className="!size-16 text-zinc-400 dark:text-zinc-600" />

      <p className="text-sm text-zinc-500 dark:text-zinc-400">
        Profile Inspect — coming soon
      </p>
    </div>
  );
}
