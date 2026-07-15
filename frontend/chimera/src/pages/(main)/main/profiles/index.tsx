/**
 * 配置文件页面
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/index.tsx`
 *
 * 职责：
 * - 显示所有订阅/本地配置文件列表
 * - 支持 Profile 链式编辑（Global Proxy Chains / Per-profile Chains）
 * - 快速导入订阅链接
 * - 更新全部配置、新增配置（浮动操作按钮）
 * - 运行时配置差异查看
 *
 * 当前阶段（Step 5）：
 * - 使用现有的 Chimera 配置文件组件
 * - 适配 (main) 布局（使用 AppContentScrollArea 替代 SidePage）
 * - 保留链编辑侧栏（ProfileSide）覆盖层
 * - 保留浮动操作按钮（FAB）
 * - 移除 SidePage 包装，改用扁平布局
 *
 * 后续迁移计划：
 * - 迁移到 ref 的配置文件组件实现
 * - 添加拖拽排序支持
 * - 添加配置搜索过滤
 */

import { RemoteProfileOptionsBuilder, useProfile } from '@chimera/interface';
import {
  ErrorOutlined,
  Public,
  TextSnippetOutlined,
  Update,
} from '@mui/icons-material';
import {
  Badge,
  Button,
  CircularProgress,
  Fab,
  Grid,
  IconButton,
} from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { useAtom } from 'jotai';
import { AnimatePresence, motion } from 'motion/react';
import { useMemo, useState, useTransition } from 'react';
import { useWindowSize } from 'react-use';
import ContentDisplay from '@/components/base/content-display';
import {
  atomChainsSelected,
  atomGlobalChainCurrent,
} from '@/components/profiles/modules/store';
import NewProfileButton from '@/components/profiles/new-profile-button';
import {
  AddProfileContext,
  type AddProfileContextValue,
} from '@/components/profiles/profile-dialog';
import ProfileItem from '@/components/profiles/profile-item';
import ProfileSide from '@/components/profiles/profile-side';
import { GlobalUpdatePendingContext } from '@/components/profiles/provider';
import { QuickImport } from '@/components/profiles/quick-import';
import RuntimeConfigDiffDialog from '@/components/profiles/runtime-config-diff-dialog';
import { ClashProfile, filterProfiles } from '@/components/profiles/utils';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

/**
 * 注册 TanStack Router 文件路由
 * 路径: `/main/profiles/`（index 子路由）
 * 匹配 ref: `createFileRoute('/(main)/main/profiles/')`
 * URL 参数已在父路由（route.tsx）中验证
 */
export const Route = createFileRoute('/(main)/main/profiles/')({
  component: ProfilePage,
});

type RemoteProfileItem = Extract<ClashProfile, { type: 'remote' }>;

/**
 * 配置文件列表页主组件
 *
 * 在 (main) 布局下提供：
 * - 配置文件网格（支持 local/remote 类型）
 * - 链式编辑面板（Global Proxy Chains / Per-profile Chains）
 * - 快速导入区域
 * - 浮动操作按钮（更新全部 / 新增配置）
 * - 运行时配置查看器
 */
function ProfilePage() {
  const { query, update } = useProfile();
  const search = Route.useSearch();

  // 过滤后的配置文件列表（仅 clash 类型）
  const profiles = useMemo(() => {
    return filterProfiles(query.data?.items);
  }, [query.data?.items]);
  const profileItems = profiles?.clash ?? [];

  // 从 URL search 参数中提取订阅链接，用于预填 AddProfileDialog
  const addProfileCtxValue = useMemo(
    () =>
      search.subscribeUrl
        ? ({
            name: search.subscribeName ?? null,
            desc: search.subscribeDesc ?? null,
            url: search.subscribeUrl,
          } satisfies AddProfileContextValue)
        : null,
    [search],
  );

  // 链式编辑状态管理
  const [globalChain, setGlobalChain] = useAtom(atomGlobalChainCurrent);
  const [chainsSelected, setChainsSelected] = useAtom(atomChainsSelected);

  const handleChainsClick = (profile: ClashProfile) => {
    setGlobalChain(false);

    if (chainsSelected === profile.uid) {
      setChainsSelected(undefined);
    } else {
      setChainsSelected(profile.uid);
    }
  };

  const handleGlobalChainClick = () => {
    setChainsSelected(undefined);
    setGlobalChain(!globalChain);
  };

  // 侧栏面板是否处于显示状态
  const hasSide = globalChain || chainsSelected;

  const handleSideClose = () => {
    setChainsSelected(undefined);
    setGlobalChain(false);
  };

  // 运行时配置查看器
  const [runtimeConfigViewerOpen, setRuntimeConfigViewerOpen] = useState(false);
  const { width } = useWindowSize();

  // 全局更新配置状态
  const [globalUpdatePending, startGlobalUpdate] = useTransition();

  /**
   * 更新所有远程配置
   * 遍历 profiles 中所有 remote 类型配置，逐个发起更新
   */
  const handleGlobalProfileUpdate = useLockFn(async () => {
    startGlobalUpdate(async () => {
      const remoteProfiles =
        (profiles?.clash?.filter(
          (item) => item.type === 'remote',
        ) as RemoteProfileItem[]) || [];

      const updates: Array<Promise<void | null | undefined>> = [];

      for (const profile of remoteProfiles) {
        const option = {
          ...profile.option,
          update_interval_minutes: 0,
          user_agent: profile.option?.user_agent ?? null,
        } satisfies RemoteProfileOptionsBuilder;

        const result = await update.mutateAsync({
          uid: profile.uid,
          option,
        });
        updates.push(Promise.resolve(result));
      }

      try {
        await Promise.all(updates);
      } catch (error) {
        message(`failed to update profiles: \n${formatError(error)}`, {
          kind: 'error',
          title: m.common_error(),
        });
      }
    });
  });

  /**
   * 根据加载/错误/空状态渲染不同内容
   */
  const renderProfilesContent = () => {
    // 加载中
    if (query.isLoading) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex flex-col items-center gap-4 rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <CircularProgress size={28} />
              <div className="text-sm opacity-80">{'Loading profiles'}</div>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    // 加载出错
    if (query.isError) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex w-full max-w-md flex-col items-center rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <ErrorOutlined className="!mb-4 !size-14 text-red-400 dark:text-red-300" />
              <div className="text-base font-semibold">
                {'Failed to load profiles'}
              </div>
              <div className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
                {formatError(query.error)}
              </div>
              <Button
                className="!mt-5"
                variant="contained"
                onClick={() => query.refetch()}
              >
                {'Refresh'}
              </Button>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    // 无配置文件
    if (!profileItems.length) {
      return (
        <ContentDisplay className="px-3 pt-2 pb-4">
          <div className="flex min-h-64 w-full items-center justify-center">
            <div className="flex w-full max-w-md flex-col items-center rounded-[1.75rem] border border-black/6 bg-black/[0.025] px-8 py-10 text-center backdrop-blur-sm dark:border-white/8 dark:bg-white/[0.03]">
              <Public className="!mb-4 !size-14 text-zinc-400 dark:text-zinc-500" />
              <div className="text-base font-semibold">{'No Profiles'}</div>
              <div className="mt-2 text-sm text-zinc-500 dark:text-zinc-400">
                {'Import a subscription URL to get started'}
              </div>
            </div>
          </div>
        </ContentDisplay>
      );
    }

    // 配置文件网格
    return (
      <Grid container spacing={2}>
        {profileItems.map((item) => (
          <Grid
            key={item.uid}
            size={{
              xs: 12,
              sm: 12,
              md: hasSide && width <= 1000 ? 12 : 6,
              lg: 4,
              xl: 3,
            }}
          >
            <motion.div
              key={item.uid}
              layoutId={`profile-${item.uid}`}
              layout="position"
              initial={false}
            >
              <ProfileItem
                item={item}
                onClickChains={handleChainsClick}
                selected={query.data?.current === item.uid}
                chainsSelected={chainsSelected === item.uid}
              />
            </motion.div>
          </Grid>
        ))}
      </Grid>
    );
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col overflow-hidden"
      data-slot="profiles-container"
    >
      {/*
        profiles 内容区域（可滚动）
        不使用 SidePage 包装，(main) 布局已提供导航栏
        链编辑侧栏（ProfileSide）作为覆盖层在内容之上显示
      */}
      <div className="flex min-h-0 flex-1" data-slot="profiles-content-wrapper">
        {/* 主内容区 */}
        <div
          className="flex min-w-0 flex-1 flex-col overflow-hidden"
          data-slot="profiles-main-content"
        >
          {/*
            顶栏：运行时配置查看器 + 全局链按钮
            匹配 legacy layout 中 SidePage header 的行为
          */}
          <div
            className="bg-mixed-background flex shrink-0 items-center justify-end gap-2 px-4 py-2"
            data-slot="profiles-header"
          >
            <RuntimeConfigDiffDialog
              open={runtimeConfigViewerOpen}
              onClose={() => setRuntimeConfigViewerOpen(false)}
            />

            <IconButton
              className="h-10 w-10"
              color="inherit"
              title={'Runtime Config'}
              onClick={() => {
                setRuntimeConfigViewerOpen(true);
              }}
            >
              <TextSnippetOutlined />
            </IconButton>

            <Badge variant="dot">
              <Button
                size="small"
                variant={globalChain ? 'contained' : 'outlined'}
                onClick={handleGlobalChainClick}
                startIcon={<Public />}
              >
                {'Global Proxy Chains'}
              </Button>
            </Badge>
          </div>

          {/* 配置文件列表 */}
          <AppContentScrollArea data-slot="profiles-scroll-area">
            <AnimatePresence initial={false} mode="sync">
              <GlobalUpdatePendingContext.Provider value={globalUpdatePending}>
                <div className="flex flex-col gap-4 p-6">
                  {/* 快速导入区域 */}
                  <QuickImport />

                  {/* 配置文件网格 */}
                  {renderProfilesContent()}
                </div>
              </GlobalUpdatePendingContext.Provider>
            </AnimatePresence>
          </AppContentScrollArea>
        </div>

        {/*
          链编辑侧栏覆盖层（ProfileSide）
          当选中某个配置的链或启用全局链时显示
          覆盖在主内容区右侧
        */}
        {hasSide && (
          <div
            className="flex shrink-0 flex-col overflow-hidden"
            data-slot="profiles-side-container"
          >
            <ProfileSide onClose={handleSideClose} />
          </div>
        )}
      </div>

      {/*
        浮动操作按钮（FAB）：更新全部 + 新增配置
        固定在右下角，不随滚动移动
      */}
      <div className="pointer-events-none fixed right-8 bottom-8 z-10 flex flex-col gap-3">
        <Fab
          className="pointer-events-auto"
          color="default"
          onClick={handleGlobalProfileUpdate}
          title={'Update All Profiles'}
          disabled={
            globalUpdatePending ||
            query.isLoading ||
            query.isError ||
            !profileItems.length
          }
        >
          {globalUpdatePending ? <CircularProgress size={22} /> : <Update />}
        </Fab>

        <AddProfileContext.Provider value={addProfileCtxValue}>
          <NewProfileButton className="pointer-events-auto" />
        </AddProfileContext.Provider>
      </div>
    </div>
  );
}
