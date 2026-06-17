/**
 * 按类型过滤的配置文件页面（子路由）
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/$type/index.tsx`
 *
 * 职责：
 * - 作为 `/main/profiles/$type` 路由，按 ProfileType 显示过滤后的配置列表
 * - 显示配置类型对应的名称和图标
 * - 过滤方式与 ProfilesNavigate 侧栏中的 PROFILE_TYPE_CONDITIONS 一致
 *
 * 当前阶段（Profiles Step 2 — 子路由迁移）：
 * - 创建 $type 子路由，使侧栏导航链接可正常跳转
 * - 使用现有的 Chimera 配置列表组件（filterProfiles + ProfileItem）
 * - 仅支持 Profile 类型（显示全部 remote/local 配置）
 * - JavaScript / Lua / Merge 类型显示占位状态
 *
 * 后续计划：
 * - 添加 JavaScript/Lua 脚本配置的专用视图
 * - 添加 Merge 配置的专用视图
 * - 添加配置搜索/排序功能
 */

import { useProfile } from '@chimera/interface';
import { Public } from '@mui/icons-material';
import { Grid } from '@mui/material';
import { AnimatePresence, motion } from 'framer-motion';
import { createFileRoute } from '@tanstack/react-router';
import { useMemo } from 'react';
import ProfileItem from '@/components/profiles/profile-item';
import { filterProfiles } from '@/components/profiles/utils';
import { ProfileType, PROFILE_TYPE_NAMES } from '../_modules/consts';

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  component: RouteComponent,
});

function RouteComponent() {
  const { type } = Route.useParams();

  const { query } = useProfile();

  // 过滤后的配置文件列表
  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);

  const profileItems = profiles?.clash ?? [];

  const profileType = type as ProfileType;
  const typeName = PROFILE_TYPE_NAMES[profileType] ?? type;

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-slot="profiles-type-container"
    >
      {/* 类型标题 */}
      <div
        className="bg-mixed-background flex shrink-0 items-center px-4 py-4"
        data-slot="profiles-type-header"
      >
        <h2 className="text-lg font-bold">{typeName}</h2>

        <span className="ml-2 text-sm text-zinc-500">
          ({profileItems.length} profiles)
        </span>
      </div>

      {/* 配置网格 */}
      <div className="flex-1 overflow-y-auto p-4 pt-0" data-slot="profiles-type-list">
        {profileItems.length > 0 ? (
          <Grid container spacing={2}>
            {profileItems.map((item) => (
              <Grid
                key={item.uid}
                size={{
                  xs: 12,
                  sm: 12,
                  md: 6,
                  lg: 4,
                  xl: 3,
                }}
              >
                <motion.div
                  layoutId={`profile-${item.uid}`}
                  layout="position"
                  initial={false}
                >
                  <ProfileItem
                    item={item}
                    onClickChains={() => {}}
                    selected={query.data?.current?.includes(item.uid)}
                    chainsSelected={false}
                  />
                </motion.div>
              </Grid>
            ))}
          </Grid>
        ) : (
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex flex-col items-center gap-4 rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <Public className="!mb-2 !size-14 text-zinc-400 dark:text-zinc-500" />
              <div className="text-base font-semibold">{typeName}</div>
              <div className="text-sm text-zinc-500 dark:text-zinc-400">
                No {typeName.toLowerCase()} profiles
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
