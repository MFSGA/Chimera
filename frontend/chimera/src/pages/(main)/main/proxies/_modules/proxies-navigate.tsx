/**
 * 代理组导航侧栏
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/_modules/proxies-navigate.tsx`
 *
 * 职责：
 * - 在桌面端侧边栏中展示所有代理组列表
 * - 每个组可作为链接点击，导航到 `/main/proxies/group/$name`
 * - 当组有图标时显示图标
 *
 * 当前阶段（Step 2）：
 * - 使用现有的 Chimera GroupList 组件渲染组列表
 * - 后续可逐步替换为 ref 的虚拟化列表实现
 */

import { useClashProxies } from '@chimera/interface';
import { cn, LazyImage } from '@chimera/ui';
import { Link, useLocation } from '@tanstack/react-router';
import { Button } from '@/components/ui/button';

/**
 * 代理组导航组件
 *
 * 渲染在 SidebarContent 中，显示所有代理组列表。
 * 每个组是一个按钮，点击跳转到对应的组详情页。
 */
export default function ProxiesNavigate() {
  const {
    proxies: { data: proxies },
  } = useClashProxies();

  const location = useLocation();

  return (
    <div className="flex flex-col gap-2 p-2" data-slot="proxies-navigate">
      {proxies?.groups.map((group) => (
        <Button
          key={group.name}
          variant="fab"
          data-active={String(
            location.pathname.endsWith(`/group/${group.name}`),
          )}
          asChild
        >
          <Link
            className={cn(
              'h-16',
              'flex items-center gap-2',
              'data-[active=true]:bg-surface-variant/80',
              'data-[active=false]:bg-transparent',
              'data-[active=false]:shadow-none',
              'data-[active=false]:hover:shadow-none',
              'data-[active=false]:hover:bg-surface-variant/30',
            )}
            to="/main/proxies/$name"
            params={{
              name: group.name,
            }}
            search={(prev) => ({
              searchQuery: prev.searchQuery,
            })}
          >
            <div className="flex items-center gap-2.5">
              {/* 代理组图标（如果有） */}
              {group.icon && (
                <div className="size-8">
                  <LazyImage
                    src={group.icon}
                    className="size-8"
                    loadingClassName="rounded-full"
                  />
                </div>
              )}

              <div className="flex flex-col gap-1">
                <div className="text-sm font-medium">{group.name}</div>
                <div className="text-xs text-zinc-500">
                  {group.now || group.type}
                </div>
              </div>
            </div>
          </Link>
        </Button>
      ))}
    </div>
  );
}
