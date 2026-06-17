/**
 * Dashboard 快捷操作 Widget
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/widget-shortcut.tsx`
 *
 * 两个 Widget 组件：
 * 1. ProxyShortcutsWidget — 系统代理 / TUN 模式快捷开关
 * 2. CoreShortcutsWidget — 当前运行核心状态卡片
 *
 * 适配说明：
 * - 使用 Chimera 的 PaperSwitchButton 替代 ref 的 SystemProxyButton/TunModeButton
 * - 核心状态显示使用 Chimera 的 getCoreStatus 接口
 * - 使用 div 基础卡片样式替代 ref 的 Card/CardHeader/CardContent 组件
 * - 使用 @chimera/interface 的 hooks 替代 @nyanpasu/interface
 */

import NetworkPing from '~icons/material-symbols/network-ping';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import { useMemo } from 'react';
import { PaperSwitchButton } from '@/components/setting/modules/system-proxy';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import { cn } from '@chimera/ui';
import { m } from '@/paraglide/messages';
import {
  type CoreState,
  useClashConfig,
  useClashCores,
  useSetting,
  useSystemProxy,
  useSystemService,
} from '@chimera/interface';
import { getCoreStatus } from '@chimera/interface';
import { Link } from '@tanstack/react-router';
import useSWR from 'swr';
import type { WidgetComponentProps } from './consts';
import WidgetItem from './widget-item';

/** 代理状态枚举 */
enum ProxyStatus {
  SYSTEM = 'system',
  TUN = 'tun',
  OCCUPIED = 'occupied',
  DISABLED = 'disabled',
}

/**
 * 代理状态标题行
 * 显示当前系统代理/TUN 模式状态和相应颜色的标签
 */
const ProxyTitleRow = () => {
  const { value: enableSystemProxy } = useSetting('enable_system_proxy');
  const { value: enableTunMode } = useSetting('enable_tun_mode');
  const { data: systemProxyStatus } = useSystemProxy();
  const {
    query: { data: clashConfigs },
  } = useClashConfig();

  const status = useMemo<ProxyStatus>(() => {
    if (enableTunMode) {
      return ProxyStatus.TUN;
    }

    if (enableSystemProxy) {
      if (systemProxyStatus?.enable) {
        const port = Number(systemProxyStatus.server.split(':')[1]);

        if (port === clashConfigs?.['mixed-port']) {
          return ProxyStatus.SYSTEM;
        }

        return ProxyStatus.OCCUPIED;
      }
    }

    return ProxyStatus.DISABLED;
  }, [enableSystemProxy, enableTunMode, systemProxyStatus, clashConfigs]);

  const messages = {
    [ProxyStatus.SYSTEM]: '系统代理',
    [ProxyStatus.TUN]: 'TUN 模式',
    [ProxyStatus.OCCUPIED]: '占用',
    [ProxyStatus.DISABLED]: '已禁用',
  };

  return (
    <div className="flex items-center gap-3 px-1">
      <span className="shrink-0 font-bold">
        {m.dashboard_widget_proxy_status()}
      </span>

      <Button
        variant="raised"
        className={cn(
          'flex h-6 min-w-0 items-center px-0',
          status === ProxyStatus.DISABLED &&
            'bg-secondary-container hover:bg-on-secondary',
          status === ProxyStatus.OCCUPIED &&
            'bg-error-container hover:bg-on-error',
          status === ProxyStatus.SYSTEM &&
            'bg-primary-container hover:bg-on-primary',
          status === ProxyStatus.TUN &&
            'bg-tertiary-container hover:bg-on-tertiary',
        )}
        asChild
      >
        <Link to="/main/settings">
          <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
            {messages[status]}
          </TextMarquee>
        </Link>
      </Button>
    </div>
  );
};

/**
 * ProxyShortcutsWidget — 系统代理 / TUN 模式快捷开关
 *
 * 显示：
 * - 当前代理状态标题
 * - 系统代理开关按钮
 * - TUN 模式开关按钮
 *
 * 使用 PaperSwitchButton 组件实现开关，与 Chimera 设置页面保持一致。
 */
export function ProxyShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  const systemProxy = useSetting('enable_system_proxy');
  const tunMode = useSetting('enable_tun_mode');

  return (
    <WidgetItem id={id} minW={3} minH={2} onCloseClick={onCloseClick}>
      <div className="flex size-full flex-col justify-between rounded-3xl bg-surface-variant/30">
        <ProxyTitleRow />

        <div className="flex flex-1 gap-3 p-2">
          {/* 系统代理按钮 */}
          <PaperSwitchButton
            checked={systemProxy.value || false}
            onClick={() => systemProxy.upsert(!systemProxy.value)}
          >
            <div className="flex flex-col items-center gap-2">
              <NetworkPing className="size-6" />
              <span className="text-sm">
                {m.settings_system_proxy_system_proxy_label()}
              </span>
            </div>
          </PaperSwitchButton>

          {/* TUN 模式按钮 */}
          <PaperSwitchButton
            checked={tunMode.value || false}
            onClick={() => tunMode.upsert(!tunMode.value)}
          >
            <div className="flex flex-col items-center gap-2">
              <SettingsEthernet className="size-6" />
              <span className="text-sm">
                {m.settings_system_proxy_tun_mode_label()}
              </span>
            </div>
          </PaperSwitchButton>
        </div>
      </div>
    </WidgetItem>
  );
}

/**
 * 核心状态徽章
 * 显示核心运行状态的文本描述
 */
const CoreStatusBadge = () => {
  const {
    query: { data: serviceStatus },
  } = useSystemService();

  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  });

  const message = useMemo<string>(() => {
    const status = coreStatusSWR.data?.[0];
    const coreState = status as CoreState | undefined;
    const isRunning = coreState === 'Running';

    if (isRunning) {
      if (serviceStatus?.server?.core_infos?.state === 'Running') {
        return '核心以服务模式运行';
      }
      return '核心以子进程运行';
    }

    let stoppedMessage = '核心已停止';
    let serviceMessage = '';

    const stoppedInfo =
      coreState && typeof coreState === 'object' && 'Stopped' in coreState
        ? coreState.Stopped
        : null;

    if (serviceStatus?.status === 'running') {
      serviceMessage = '服务运行中';

      if (stoppedInfo) {
        stoppedMessage = `核心被服务停止: ${stoppedInfo}`;
      } else {
        stoppedMessage = '核心被服务停止（未知原因）';
      }
    } else if (serviceStatus?.status === 'stopped') {
      serviceMessage = '服务已停止';
    } else {
      serviceMessage = '服务未安装';
    }

    if (stoppedInfo) {
      stoppedMessage = `核心已停止: ${stoppedInfo}`;
    }

    return `${stoppedMessage} ${serviceMessage}`;
  }, [serviceStatus, coreStatusSWR.data]);

  return (
    <div
      className={cn(
        'flex h-6 min-w-0 items-center rounded-full text-sm',
        'bg-surface-variant/50',
      )}
      data-slot="core-status-badge"
    >
      <TextMarquee className="px-2" fadeEdges fadeWidth={8}>
        {message}
      </TextMarquee>
    </div>
  );
};

/**
 * 当前核心卡片
 * 显示当前选中的核心名称、版本和运行状态
 */
const CurrentCoreCard = () => {
  const { query: clashCores } = useClashCores();
  const { value: currentCoreKey } = useSetting('clash_core');
  const coreStatusSWR = useSWR('/coreStatus', getCoreStatus, {
    refreshInterval: 2000,
    revalidateOnFocus: false,
  });

  const status = coreStatusSWR.data?.[0] as CoreState | undefined;
  const isRunning = status === 'Running';
  const currentCore = currentCoreKey ? clashCores.data?.[currentCoreKey] : null;

  return (
    <Button
      variant="raised"
      className={cn(
        'group flex flex-1 items-center gap-4 rounded-2xl pr-3 pl-4',
        'bg-surface-variant/30 hover:bg-surface-variant',
      )}
      data-running={String(isRunning)}
      data-slot="current-core-card"
      asChild
    >
      <Link to="/main/settings">
        {/* 核心图标区域 */}
        <div
          className="flex size-12 shrink-0 items-center justify-center rounded-full bg-surface-variant"
          data-slot="core-icon"
        >
          <SettingsEthernet className="size-6" />
        </div>

        <div
          className="flex flex-1 flex-col items-start gap-1 truncate"
          data-slot="core-info"
        >
          <div className="font-semibold" data-slot="core-name">
            {currentCore?.name ?? currentCoreKey ?? '未选择'}
          </div>

          <div
            className="text-zinc-700 dark:text-zinc-300"
            data-slot="core-version"
          >
            {currentCore?.currentVersion ?? '-'}
          </div>
        </div>

        <div
          className="flex items-center gap-2 truncate pr-2"
          data-slot="core-status"
        >
          <div className="truncate" data-slot="core-status-text">
            {isRunning ? '运行中' : '已停止'}
          </div>

          <div
            className="relative flex size-3 shrink-0"
            data-slot="core-status-indicator"
          >
            <span
              className={cn(
                'absolute inline-flex size-full animate-ping rounded-full opacity-75',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:opacity-0',
              )}
            />
            <span
              className={cn(
                'relative inline-flex size-full rounded-full',
                'group-data-[running=true]:bg-green-500',
                'group-data-[running=false]:bg-gray-400',
              )}
            />
          </div>
        </div>
      </Link>
    </Button>
  );
};

/**
 * CoreShortcutsWidget — 当前运行核心状态卡片
 *
 * 显示：
 * - 核心状态徽章（运行中/已停止/服务状态）
 * - 当前核心的详细信息卡片（名称、版本、运行状态指示器）
 * - 点击进入设置-核心页面
 */
export function CoreShortcutsWidget({
  id,
  onCloseClick,
}: WidgetComponentProps) {
  return (
    <WidgetItem id={id} minW={4} minH={2} onCloseClick={onCloseClick}>
      <div className="flex size-full flex-col justify-between rounded-3xl bg-surface-variant/30 p-2">
        <div className="flex items-center gap-3 px-1">
          <span className="shrink-0 font-bold">
            {m.dashboard_widget_core_status()}
          </span>

          <CoreStatusBadge />
        </div>

        <div className="flex-1 pt-2">
          <CurrentCoreCard />
        </div>
      </div>
    </WidgetItem>
  );
}
