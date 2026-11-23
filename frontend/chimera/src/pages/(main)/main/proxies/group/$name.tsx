/**
 * 代理组详情页（子路由）
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/group/$name.tsx`
 * 路由结构与 ref 保持一致：`/main/proxies/group/$name`
 *
 * 职责：
 * - 作为 `/main/proxies` 的子路由，在 Sidebar 布局的右侧内容区显示
 * - 根据路由参数 `name` 查找并显示指定代理组的节点列表
 * - 使用 @tanstack/react-virtual 多列虚拟化网格布局（lanes）
 * - 提供延迟测试、节点选择、组流量显示等功能
 *
 * 当前阶段（Proxies Step 3 — 组详情迁移至 ref）：
 * - 替换 virtua 的 NodeList 为 @tanstack/react-virtual 多列网格（lanes）
 * - 替换 MUI DelayButton 为 ref 的 DelayTestButton（含加载动画、成功反馈）
 * - 添加 GroupHeader（组名 + 流量统计 + 滚动到当前节点）
 * - 添加 ProxyNodeButton（单节点选择 + 延迟测试）
 * - 使用 useContainerBreakpointValue 响应式调整列数（xs~xl）
 * - 使用 useCurrentGroupConnection 显示组流量（download/upload）
 */

import {
  nativeMemberState,
  resolveProxyChain,
  useKvStorage,
  useNativeProxies,
  useNativeProxyActions,
  useProxyMode,
  type Proxy_Serialize,
  type ProxyGroup,
} from '@chimera/interface';
import { useContainerBreakpointValue } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import {
  useScrollAreaViewport,
  useScrollAreaViewportElement,
} from '@chimera/ui/scroll-area';
import { createFileRoute } from '@tanstack/react-router';
import { useVirtualizer } from '@tanstack/react-virtual';
import KeepOffRounded from '~icons/material-symbols/keep-off-rounded';
import KeepRounded from '~icons/material-symbols/keep-rounded';
import {
  AnimatePresence,
  motion,
  useReducedMotion,
  type Transition,
} from 'motion/react';
import {
  useCallback,
  useDeferredValue,
  useMemo,
  useRef,
  useState,
} from 'react';
import { Notice } from '@/components/base';
import { DelayHistoryTooltip } from '@/components/proxies/delay-history';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { useSearchTerm } from '../../_modules/use-search-term';
import DelayTestButton from './_modules/delay-test-button';
import GroupHeader from './_modules/group-header';
import {
  GroupMeta,
  GroupStatus,
  GroupTrafficSpeed,
} from './_modules/group-meta';
import {
  DEFAULT_NODE_VIEW,
  NODE_VIEW_KV_KEY,
  toNodeView,
  visibleMembers,
  type NodeView,
} from './_modules/node-list';
import {
  HideUnavailableButton,
  LocateCurrentNodeButton,
  NodeSearchOverlay,
  NoMatchingNodes,
  SearchNodesButton,
  SortNodesButton,
} from './_modules/node-list-toolbar';
import ProxyNodeButton from './_modules/proxy-node-button';

/**
 * 注册 TanStack Router 文件路由
 * 此文件位于 `proxies/group/$name.tsx`，作为 `/(main)/main/proxies` 的子路由，
 * 路径模板: `/main/proxies/group/$name`
 *
 * 子路由会被渲染在 parent route.tsx 的 `<Outlet />` / `<AnimatedOutletPreset />` 中，
 * 因此组详情会显示在 Sidebar 的右侧内容区域。
 */
export const Route = createFileRoute('/(main)/main/proxies/group/$name')({
  component: RouteComponent,
});

/**
 * 代理组详情页组件
 *
 * 布局结构（匹配 ref）：
 * - GroupHeader：sticky 头部（组名 + 流量统计 + 滚动到当前节点按钮）
 * - Virtualized Grid：多列虚拟化网格（每个 ProxyNodeButton）
 * - DelayTestButton：浮动延迟测试按钮
 *
 */
function RouteComponent() {
  // 从路由参数中获取代理组名称
  const { name: proxyGroupName } = Route.useParams();
  const { q } = Route.useSearch();
  const navigate = Route.useNavigate();
  const writeQuery = useCallback(
    (next: string | undefined) =>
      navigate({
        search: (previous) => ({ ...previous, q: next }),
        replace: true,
      }),
    [navigate],
  );
  const [search, setSearch] = useSearchTerm(q, writeQuery);
  const deferredSearch = useDeferredValue(search);
  const [searchOpen, setSearchOpen] = useState(Boolean(q));
  const headerRef = useRef<HTMLDivElement>(null);
  const [searchOrigin, setSearchOrigin] = useState(0.5);
  const [searchOverlayLeft, setSearchOverlayLeft] = useState(0);
  const reduceMotion = useReducedMotion();
  const headerFade: Transition = reduceMotion
    ? { duration: 0 }
    : { duration: 0.2, ease: 'easeOut' };
  const searchOverlayClip = `inset(0% ${(1 - searchOrigin) * 100}% 0% ${searchOrigin * 100}%)`;

  const handleSearchOpenChange = useCallback(
    (open: boolean) => {
      setSearchOpen(open);
      if (!open) {
        // Do not keep an invisible URL filter active after closing search.
        setSearch('');
        return;
      }

      const header = headerRef.current;
      const button = header?.querySelector(
        '[data-slot="proxies-node-search-button"]',
      );
      const row = header?.querySelector(
        '[data-slot="proxies-group-header-row"]',
      );
      if (!button || !row) return;
      const rowRect = row.getBoundingClientRect();
      if (!rowRect.width) return;
      const buttonRect = button.getBoundingClientRect();
      const back = header?.querySelector(
        '[data-slot="proxies-group-back-button"]',
      );
      const overlayLeft = back
        ? back.getBoundingClientRect().right - rowRect.left
        : 0;
      const overlayWidth = rowRect.width - overlayLeft;
      const center = buttonRect.left + buttonRect.width / 2 - rowRect.left;
      setSearchOverlayLeft(overlayLeft);
      setSearchOrigin(
        overlayWidth > 0
          ? Math.min(Math.max(center / overlayWidth, 0), 1)
          : 0.5,
      );
    },
    [setSearch],
  );
  const [storedView, setView] = useKvStorage<NodeView>(
    NODE_VIEW_KV_KEY,
    DEFAULT_NODE_VIEW,
    { migrate: toNodeView },
  );
  const handleViewChange = useCallback(
    async (next: NodeView) => {
      if (!(await setView(next))) {
        Notice.error(m.proxies_node_view_save_failed(), 5000);
      }
    },
    [setView],
  );

  const { data: proxies } = useNativeProxies();
  const { selectProxy, clearProxyFixed, isClearingFixed, updateProxiesDelay } =
    useNativeProxyActions();
  const { mutateAsync: mutateProxyDelay } = updateProxiesDelay;

  const { value: proxyMode } = useProxyMode();

  // 查找当前选中的代理组（或 global 模式的 special 组）
  const currentGroup = useMemo<ProxyGroup | undefined>(() => {
    if (proxyMode.global) {
      return proxies?.global ?? undefined;
    }

    return proxies?.groups.find((group) => group.name === proxyGroupName);
  }, [proxies, proxyGroupName, proxyMode]);

  const groupName = currentGroup?.name;
  const selectable = currentGroup?.capabilities.select ?? false;

  const handleClearFixed = useLockFn(async () => {
    if (
      !groupName ||
      !currentGroup?.fixed ||
      !currentGroup.capabilities.clearFixed
    ) {
      return;
    }
    try {
      await clearProxyFixed(groupName);
    } catch {
      // Ref: a rejected mutation leaves the fixed selection unchanged and
      // reports a localized failure instead of exposing raw backend details.
      Notice.error(
        m.proxies_clear_fixed_failed_message({ group: groupName }),
        5000,
      );
    }
  });

  const handleSelectProxy = useCallback(
    async (proxy: Proxy_Serialize) => {
      if (!groupName || !selectable) return;
      try {
        await selectProxy(groupName, proxy.name);
      } catch {
        Notice.error(
          m.proxies_select_failed_message({
            group: groupName,
            name: proxy.name,
          }),
          5000,
        );
      }
    },
    [groupName, selectable, selectProxy],
  );

  const handleDelayTest = useCallback(
    async (proxy: Proxy_Serialize) => {
      if (!groupName) return;
      await mutateProxyDelay([groupName, proxy.name]);
    },
    [mutateProxyDelay, groupName],
  );

  // 获取 AppContentScrollArea 的 viewportRef（用于同步虚拟滚动）
  const { viewportRef } = useScrollAreaViewport();
  const viewportElement = useScrollAreaViewportElement();

  // 响应式列数：根据容器宽度自动调整网格列数（匹配 ref）
  const lanes = useContainerBreakpointValue(
    viewportRef,
    {
      xs: 2,
      sm: 3,
      md: 4,
      lg: 5,
      xl: 6,
    },
    4,
  );

  const narrowHeader = useContainerBreakpointValue(
    viewportRef,
    { xs: true, sm: false },
    false,
  );

  // Ref computes each member's resolved leaf/history once per graph snapshot.
  const memberStates = useMemo(() => {
    if (!currentGroup || !proxies) return new Map();
    return new Map(
      currentGroup.all.map((name) => [
        name,
        nativeMemberState(name, currentGroup, proxies),
      ]),
    );
  }, [currentGroup, proxies]);

  const groupStatus = useMemo(() => {
    if (!currentGroup || !proxies) return undefined;
    let available = 0;
    for (const { delay } of memberStates.values()) {
      if ((delay ?? 0) > 0) available += 1;
    }
    return {
      available,
      total: currentGroup.all.length,
      chain: resolveProxyChain(currentGroup.name, proxies).path,
    };
  }, [currentGroup, proxies, memberStates]);

  const members = useMemo(
    () =>
      currentGroup && proxies
        ? visibleMembers({
            group: currentGroup,
            proxies,
            query: deferredSearch,
            view: storedView,
            delayOf: (name) => memberStates.get(name)?.delay,
          })
        : [],
    [currentGroup, proxies, deferredSearch, storedView, memberStates],
  );

  // HighlightText supports one substring; multi-term searches only filter.
  const trimmedSearch = deferredSearch.trim();
  const searchText = trimmedSearch.includes(' ') ? '' : trimmedSearch;

  // Ref's cards are fixed-height: measuring each visible card forces layout.
  const virtualizer = useVirtualizer({
    count: members.length,
    getScrollElement: () => viewportElement,
    estimateSize: () => 64,
    overscan: 5,
    lanes,
  });

  const virtualItems = virtualizer.getVirtualItems();
  // Match ref: allow the route transition to paint before mounting large grids.
  const showNodes = useDeferredValue(true, false);
  const currentIndex = currentGroup?.now
    ? members.indexOf(currentGroup.now)
    : -1;
  const handleScrollToCurrentNode = useCallback(() => {
    if (currentIndex >= 0) {
      virtualizer.scrollToIndex(currentIndex, {
        align: 'center',
        behavior: 'smooth',
      });
    }
  }, [currentIndex, virtualizer]);
  const noMatch = members.length === 0 && Boolean(currentGroup?.all.length);

  return (
    <>
      {/*
        GroupHeader：sticky 头部
        包含组名称、流量统计、滚动到当前节点按钮
      */}
      <GroupHeader ref={headerRef}>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          {currentGroup && groupStatus ? (
            <GroupMeta
              name={currentGroup.name}
              status={
                <GroupStatus
                  available={groupStatus.available}
                  total={groupStatus.total}
                  chain={groupStatus.chain}
                />
              }
              speed={<GroupTrafficSpeed groupName={currentGroup.name} />}
              speedCompact={
                <GroupTrafficSpeed
                  groupName={currentGroup.name}
                  only="fastest"
                />
              }
            />
          ) : (
            <span className="min-w-0 truncate font-medium">
              {currentGroup?.name}
            </span>
          )}

          {currentGroup?.fixed && (
            <div
              className="text-on-surface-variant flex min-w-0 items-center gap-1 text-xs"
              title={currentGroup.fixed}
              data-slot="proxies-group-fixed"
            >
              <KeepRounded className="size-3.5 shrink-0" />
              <span className="shrink-0">{m.proxies_group_fixed_label()}</span>
              <span className="truncate">{currentGroup.fixed}</span>
            </div>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <SearchNodesButton
            search={search}
            onSearchChange={setSearch}
            open={narrowHeader ? false : searchOpen}
            onOpenChange={handleSearchOpenChange}
          />
          {currentGroup?.fixed && currentGroup.capabilities.clearFixed && (
            <Button
              variant="stroked"
              className="flex h-8 shrink-0 items-center gap-1 px-3 text-sm"
              data-slot="proxies-group-clear-fixed-button"
              disabled={isClearingFixed}
              onClick={handleClearFixed}
            >
              <KeepOffRounded className="size-4" />
              <span>{m.proxies_group_clear_fixed_button()}</span>
            </Button>
          )}
          <SortNodesButton
            view={storedView}
            onViewChange={(next) => void handleViewChange(next)}
          />
          <HideUnavailableButton
            pressed={storedView.hideUnavailable}
            onPressedChange={(hideUnavailable) =>
              void handleViewChange({ ...storedView, hideUnavailable })
            }
          />
          <LocateCurrentNodeButton
            disabled={currentIndex === -1}
            onLocate={handleScrollToCurrentNode}
          />
        </div>

        <AnimatePresence initial={false}>
          {narrowHeader && searchOpen && (
            <motion.div
              key="proxies-search-overlay"
              className="bg-mixed-background absolute inset-y-0 right-0 z-20 flex items-center px-1"
              data-slot="proxies-node-search-overlay"
              style={{ left: searchOverlayLeft }}
              initial={{ clipPath: searchOverlayClip, opacity: 0.4 }}
              animate={{ clipPath: 'inset(0% 0% 0% 0%)', opacity: 1 }}
              exit={{ clipPath: searchOverlayClip, opacity: 0.4 }}
              transition={headerFade}
            >
              <NodeSearchOverlay
                search={search}
                onSearchChange={setSearch}
                onClose={() => handleSearchOpenChange(false)}
              />
            </motion.div>
          )}
        </AnimatePresence>
      </GroupHeader>

      {/*
        虚拟化网格容器
        使用 @tanstack/react-virtual 的多列 lanes 布局
      */}
      {noMatch ? (
        <NoMatchingNodes />
      ) : (
        <DelayHistoryTooltip>
          <div
            className="relative m-2"
            data-slot="proxies-virtual-list"
            style={{
              width: 'calc(100% - 16px)',
              height: `${virtualizer.getTotalSize()}px`,
            }}
          >
            {showNodes &&
              virtualItems.map((virtualItem) => {
                const proxyName = members[virtualItem.index];
                const proxy = proxyName ? proxies?.nodes[proxyName] : undefined;
                const state = proxyName
                  ? memberStates.get(proxyName)
                  : undefined;

                if (!proxy || !state) {
                  return null;
                }

                return (
                  <div
                    key={proxyName}
                    className="group absolute top-0 left-0 p-1"
                    style={{
                      height: `${virtualItem.size}px`,
                      transform: `translateY(${virtualItem.start}px)`,
                      width: `${100 / lanes}%`,
                      left: `${virtualItem.lane * (100 / lanes)}%`,
                    }}
                    data-index={virtualItem.index}
                    data-slot="proxies-virtual-item"
                    data-active={String(proxy.name === currentGroup?.now)}
                  >
                    <ProxyNodeButton
                      proxy={proxy}
                      history={state.history}
                      delay={state.delay}
                      leaf={state.leaf}
                      selectable={selectable}
                      searchText={searchText}
                      fixed={currentGroup?.fixed === proxy.name}
                      onSelect={handleSelectProxy}
                      onDelayTest={handleDelayTest}
                    />
                  </div>
                );
              })}
          </div>
        </DelayHistoryTooltip>
      )}

      {/* 浮动延迟测试按钮 */}
      <DelayTestButton groupName={currentGroup?.name} />
    </>
  );
}
