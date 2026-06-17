/**
 * Dashboard Sparkline 趋势图 Widget
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/widget-sparkline.tsx`
 *
 * 四个 Widget 组件，使用 Sparkline 趋势图展示实时数据：
 * 1. TrafficDownWidget — 下行流量趋势（附带总流量）
 * 2. TrafficUpWidget — 上行流量趋势（附带总流量）
 * 3. ConnectionsWidget — 连接数趋势
 * 4. MemoryWidget — 内存使用趋势
 *
 * 使用 @chimera/interface 的 hooks 获取实时数据：
 * - useClashTraffic：上下行流量数据（每秒更新）
 * - useClashConnections：连接数和总流量汇总
 * - useClashMemory：内存使用数据（mihomo 核心时可用）
 *
 * 适配说明：
 * - 使用 div 基础卡片样式替代 ref 的 Card/CardHeader/CardContent 组件
 * - Sparkline 组件已迁移至 @/components/ui/sparkline
 * - 接口包路径改为 @chimera/interface
 */

import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded';
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded';
import { filesize } from 'filesize';
import type { ComponentProps, ComponentType } from 'react';
import { Sparkline } from '@/components/ui/sparkline';
import TextMarquee from '@/components/ui/text-marquee';
import { cn } from '@chimera/ui';
import {
  type ClashConnection,
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@chimera/interface';
import * as m from '@/paraglide/messages';
import type { WidgetComponentProps } from './consts';
import WidgetItem from './widget-item';

/**
 * 填充数据数组
 * 当数据不足 max 长度时，在数组前补零以达到 max 长度
 */
const padData = (data: (number | undefined)[] = [], max: number) =>
  Array(Math.max(0, max - data.length))
    .fill(0)
    .concat(data.slice(-max));

/**
 * SparklineCard — 趋势图卡片容器
 *
 * 包装 WidgetItem 和 Sparkline 组件，提供：
 * - 背景趋势图（Sparkline 占满整个卡片背景）
 * - 前景内容（标题、数值、底部信息）在 z-index 上层
 */
function SparklineCard({
  id,
  minH = 2,
  minW = 2,
  maxW,
  maxH,
  data,
  className,
  children,
  onCloseClick,
  ...props
}: ComponentProps<'div'> & {
  data: number[];
} & {
  id: string;
  minW?: number;
  minH?: number;
  maxW?: number;
  maxH?: number;
  onCloseClick?: (id: string) => void;
}) {
  return (
    <WidgetItem
      id={id}
      minH={minH}
      minW={minW}
      maxW={maxW}
      maxH={maxH}
      onCloseClick={onCloseClick}
    >
      <div
        className={cn('relative isolate size-full rounded-3xl', className)}
        data-slot="widget-sparkline-card"
        {...props}
      >
        {/* 背景 Sparkline 趋势图 */}
        <Sparkline data={data} className="absolute inset-0 z-0 rounded-3xl" />

        {/* 前景内容层 */}
        <div
          className="relative z-10 flex size-full flex-col justify-between gap-1 p-3"
          data-slot="widget-sparkline-card-content"
        >
          {children}
        </div>
      </div>
    </WidgetItem>
  );
}

/**
 * SparklineCardTitle — 标题行（图标 + 文本）
 */
function SparklineCardTitle({
  icon: Icon,
  className,
  children,
  ...props
}: ComponentProps<'div'> & {
  icon: ComponentType<{ className?: string }>;
}) {
  return (
    <div
      className={cn('flex items-center gap-2', className)}
      data-slot="widget-sparkline-card-title"
      {...props}
    >
      <Icon className="size-5 shrink-0" />
      <TextMarquee className="font-bold">{children}</TextMarquee>
    </div>
  );
}

/**
 * SparklineCardContent — 主要数值显示
 */
function SparklineCardContent({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('text-2xl font-bold text-nowrap text-shadow-md', className)}
      data-slot="widget-sparkline-card-content"
      {...props}
    />
  );
}

/**
 * SparklineCardBottom — 底部次要信息
 */
function SparklineCardBottom({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'text-shadow-background h-5 text-sm text-nowrap text-shadow-xs',
        className,
      )}
      data-slot="widget-sparkline-card-bottom"
      {...props}
    />
  );
}

/**
 * TrafficDownWidget — 下行流量趋势
 *
 * 显示：
 * - Sparkline 趋势图（下行速率历史）
 * - 当前下行速率（filesize 格式化）
 * - 总下载量（从连接汇总数据中获取）
 */
export function TrafficDownWidget({ id, onCloseClick }: WidgetComponentProps) {
  const { data: clashTraffic } = useClashTraffic();
  const { query: connectionsQuery } = useClashConnections();
  const clashConnections = connectionsQuery.data;
  const total = clashConnections?.at(-1)?.downloadTotal;

  return (
    <SparklineCard
      id={id}
      data={padData(
        clashTraffic?.map((item) => item.down),
        MAX_TRAFFIC_HISTORY,
      )}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={ArrowDownwardRounded}>
        {m.dashboard_widget_traffic_download()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashTraffic?.at(-1)?.down ?? 0)}/s
      </SparklineCardContent>

      <SparklineCardBottom>
        {total !== undefined &&
          m.dashboard_widget_traffic_total({
            value: filesize(total),
          })}
      </SparklineCardBottom>
    </SparklineCard>
  );
}

/**
 * TrafficUpWidget — 上行流量趋势
 *
 * 显示：
 * - Sparkline 趋势图（上行速率历史）
 * - 当前上行速率（filesize 格式化）
 * - 总上传量（从连接汇总数据中获取）
 */
export function TrafficUpWidget({ id, onCloseClick }: WidgetComponentProps) {
  const { data: clashTraffic } = useClashTraffic();
  const { query: connectionsQuery } = useClashConnections();
  const clashConnections = connectionsQuery.data;
  const total = clashConnections?.at(-1)?.uploadTotal;

  return (
    <SparklineCard
      id={id}
      data={padData(
        clashTraffic?.map((item) => item.up),
        MAX_TRAFFIC_HISTORY,
      )}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={ArrowUpwardRounded}>
        {m.dashboard_widget_traffic_upload()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashTraffic?.at(-1)?.up ?? 0)}/s
      </SparklineCardContent>

      <SparklineCardBottom>
        {total !== undefined &&
          m.dashboard_widget_traffic_total({
            value: filesize(total),
          })}
      </SparklineCardBottom>
    </SparklineCard>
  );
}

/**
 * ConnectionsWidget — 连接数趋势
 *
 * 显示：
 * - Sparkline 趋势图（连接数历史）
 * - 当前连接数
 */
export function ConnectionsWidget({ id, onCloseClick }: WidgetComponentProps) {
  const { query: connectionsQuery } = useClashConnections();
  const clashConnections = connectionsQuery.data;

  return (
    <SparklineCard
      id={id}
      data={padData(
        clashConnections?.map((item: ClashConnection) => item.connections?.length ?? 0),
        MAX_CONNECTIONS_HISTORY,
      )}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={SettingsEthernetRounded}>
        {m.dashboard_widget_connections()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {clashConnections?.at(-1)?.connections?.length ?? 0}
      </SparklineCardContent>

      <SparklineCardBottom />
    </SparklineCard>
  );
}

/**
 * MemoryWidget — 内存使用趋势
 *
 * 显示：
 * - Sparkline 趋势图（内存使用历史）
 * - 当前内存使用量（filesize 格式化）
 * - 仅在 mihomo 核心支持时显示有效数据
 */
export function MemoryWidget({ id, onCloseClick }: WidgetComponentProps) {
  const { data: clashMemory } = useClashMemory();

  return (
    <SparklineCard
      id={id}
      data={padData(
        clashMemory?.map((item) => item.inuse),
        MAX_MEMORY_HISTORY,
      )}
      onCloseClick={onCloseClick}
    >
      <SparklineCardTitle icon={MemoryOutlineRounded}>
        {m.dashboard_widget_memory()}
      </SparklineCardTitle>

      <SparklineCardContent>
        {filesize(clashMemory?.at(-1)?.inuse ?? 0)}
      </SparklineCardContent>

      <SparklineCardBottom />
    </SparklineCard>
  );
}
