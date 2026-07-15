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
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import { Grid } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useMemo } from 'react';
import { filterProfiles } from '@/components/profiles/utils';
import * as m from '@/paraglide/messages';
import { ProfileType } from '../_modules/consts';
import ImportButton from './_modules/import-button';
import ProfilesHeader from './_modules/profiles-header';
import SortableProfileItem from './_modules/sortable-profile-item';

export enum Action {
  ImportLocalProfile = 'ImportLocalProfile',
}

type ProfileTypeSearch = {
  action?: Action;
};

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  validateSearch: (search): ProfileTypeSearch => ({
    action:
      search.action === Action.ImportLocalProfile
        ? Action.ImportLocalProfile
        : undefined,
  }),
  component: RouteComponent,
});

function RouteComponent() {
  const { type } = Route.useParams();

  const { query, sort } = useProfile();

  // 过滤后的配置文件列表
  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);

  const profileType = type as ProfileType;
  const profileItems =
    profileType === ProfileType.Profile ? (profiles?.clash ?? []) : [];
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-slot="profiles-type-container"
    >
      <ProfilesHeader />

      {/* 配置网格 */}
      <div
        className="flex-1 overflow-y-auto p-4 pt-0"
        data-slot="profiles-type-list"
      >
        {profileItems.length > 0 ? (
          <>
            <DragDropProvider
              onDragEnd={(event) => {
                const filteredUids = profileItems.map((item) => item.uid);
                const nextFilteredUids = move(filteredUids, event);

                if (
                  filteredUids.every(
                    (uid, index) => uid === nextFilteredUids[index],
                  )
                ) {
                  return;
                }

                const filteredSet = new Set(filteredUids);
                let cursor = 0;
                const fullOrder = (query.data?.items ?? []).map((item) =>
                  filteredSet.has(item.uid)
                    ? nextFilteredUids[cursor++]
                    : item.uid,
                );

                sort.mutate(fullOrder);
              }}
            >
              <Grid container spacing={2}>
                {profileItems.map((item, index) => (
                  <SortableProfileItem
                    key={item.uid}
                    item={item}
                    index={index}
                    disabled={sort.isPending}
                  />
                ))}
              </Grid>
            </DragDropProvider>

            <div className="flex h-16 items-center justify-center text-center text-sm text-gray-500">
              {m.profile_no_more_profiles()}
            </div>
          </>
        ) : (
          <div className="text-on-surface-variant flex min-h-full items-center justify-center text-center text-sm">
            {m.profile_empty_list_message()}
          </div>
        )}
      </div>

      {profileType === ProfileType.Profile && <ImportButton />}
    </div>
  );
}
