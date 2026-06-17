/**
 * Profiles 侧栏导航
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/_modules/profiles-navigate.tsx`
 *
 * 职责：
 * - 在 Profiles 页面的左侧侧边栏显示
 * - 按配置类型（Profile / JavaScript / Lua / Merge / Inspect）提供导航链接
 * - 显示每种类型的配置数量
 *
 * 当前阶段（Profiles Step 2 — 侧栏导航迁移）：
 * - 匹配 ref 的 ProfilesNavigate 设计
 * - 使用 material-symbols 图标替代 ref 的混合图标集（mdi/nonicons/streamline-plump）
 * - 支持使用 useProfile 获取配置数量统计
 *
 * 后续计划：
 * - 添加 ProfileType 子路由支持（$type/）
 * - 添加 Inspect 子路由支持
 */

import { useProfile } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Link, useMatchRoute } from '@tanstack/react-router';
import CallMergeRounded from '~icons/material-symbols/call-merge-rounded';
import CodeRounded from '~icons/material-symbols/code-rounded';
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded';
import JavascriptRounded from '~icons/material-symbols/javascript-rounded';
import SearchRounded from '~icons/material-symbols/search-rounded';
import { mapValues } from 'lodash-es';
import { useMemo, type ComponentProps, type ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import * as m from '@/paraglide/messages';
/**
 * 使用共享常量（ProfileType, PROFILE_TYPE_CONDITIONS）
 * 迁移自 ref: `src/pages/(main)/main/profiles/_modules/consts.ts`
 */

import { PROFILE_TYPE_CONDITIONS, ProfileType } from './consts';

/**
 * 导航按钮组件
 * - 使用 useMatchRoute 检测当前路由是否匹配
 * - 匹配时自动高亮（data-active=true）
 * - 通过 asChild 将 Button 包装为 Link
 */
const LinkButton = ({
  href,
  exact = false,
  children,
}: {
  href: string;
  exact?: boolean;
  children: ReactNode;
}) => {
  const matchRoute = useMatchRoute();

  const isActive = !!matchRoute({
    to: href,
    fuzzy: !exact,
  });

  return (
    <Button variant="fab" data-active={String(isActive)} asChild>
      <Link
        className={cn(
          'h-14',
          'flex items-center gap-2',
          'data-[active=true]:bg-surface-variant/80',
          'data-[active=false]:bg-transparent',
          'data-[active=false]:shadow-none',
          'data-[active=false]:hover:shadow-none',
          'data-[active=false]:hover:bg-surface-variant/30',
        )}
        to={href}
      >
        {children}
      </Link>
    </Button>
  );
};

/**
 * 路由配置
 * 每个配置类型对应一个导航目标（href）和图标
 */
const ROUTES = {
  [ProfileType.Profile]: {
    label: m.profile_profile_label(),
    href: '/main/profiles',
    icon: () => (
      <div className="relative">
        <DescriptionOutlineRounded className="size-8" />
      </div>
    ),
  },
  [ProfileType.JavaScript]: {
    label: m.profile_javascript_label(),
    href: '/main/profiles/javascript',
    icon: () => (
      <div className="relative">
        <JavascriptRounded className="size-8 text-amber-400 dark:text-amber-600" />
      </div>
    ),
  },
  [ProfileType.Lua]: {
    label: m.profile_lua_label(),
    href: '/main/profiles/lua',
    icon: () => (
      <div className="relative">
        <CodeRounded className="size-8 text-blue-400 dark:text-blue-600" />
      </div>
    ),
  },
  [ProfileType.Merge]: {
    label: m.profile_merge_label(),
    href: '/main/profiles/merge',
    icon: () => (
      <div className="relative">
        <CallMergeRounded className="size-8 text-orange-400 dark:text-orange-600" />
      </div>
    ),
  },
} satisfies Record<
  ProfileType,
  {
    label: string;
    href: string;
    icon: () => ReactNode;
  }
>;

/**
 * Profiles 侧栏导航组件
 *
 * 在 SidebarContent 中渲染，显示：
 * - 每种配置类型的链接（带图标和数量统计）
 * - 分隔线
 * - Inspect 链接
 */
export default function ProfilesNavigate({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const {
    query: { data: profiles },
  } = useProfile();

  // 统计每种类型的配置数量
  const counts = useMemo<Record<ProfileType, number>>(
    () =>
      mapValues(
        PROFILE_TYPE_CONDITIONS,
        (conditions) =>
          (profiles?.items ?? []).filter((profile) =>
            conditions.some(
              (condition) =>
                profile.type === condition.type &&
                (!('script_type' in condition) ||
                  ('script_type' in profile &&
                    profile.script_type === condition.script_type)),
            ),
          ).length,
      ),
    [profiles?.items],
  );

  return (
    <div className={cn('flex flex-col gap-2', className)} {...props}>
      {Object.entries(ROUTES).map(([profileType, route]) => (
        <LinkButton key={route.href} href={route.href}>
          <div className="size-8">{route.icon()}</div>

          <div className="text-sm font-medium">
            <p>{route.label}</p>

            <p className="text-xs text-zinc-500">
              {m.profile_profile_label_count({
                count: counts[profileType as ProfileType] ?? 0,
              })}
            </p>
          </div>
        </LinkButton>
      ))}

      <Separator />

      <LinkButton href="/main/profiles/inspect">
        <div className="size-8">
          <SearchRounded className="size-8" />
        </div>

        <span className="text-sm">Profile Inspect</span>
      </LinkButton>
    </div>
  );
}
