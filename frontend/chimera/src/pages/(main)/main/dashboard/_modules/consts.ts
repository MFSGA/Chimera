/**
 * Dashboard Widget 常量与类型定义
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/consts.ts`
 *
 * 定义 Dashboard DnD 网格系统的核心组件枚举、渲染映射和默认布局。
 *
 * Dashboard items 使用 12 列网格布局（CSS Grid 等价），
 * 每个 widget 指定 (x, y, w, h) 确定在网格中的位置和尺寸。
 *
 * WidgetId 枚举：
 * - TrafficDown/Up: 上下行流量趋势图（Sparkline）
 * - Connections: 连接数趋势图（Sparkline）
 * - Memory: 内存使用趋势图（Sparkline）
 * - ProxyShortcuts: 系统代理 / TUN 模式快捷开关
 * - CoreShortcuts: 当前运行核心状态卡片
 */

import type { ReactNode } from 'react';
import type { DndGridItemType } from '@/components/ui/dnd-grid';
import { CoreShortcutsWidget, ProxyShortcutsWidget } from './widget-shortcut';
import {
  ConnectionsWidget,
  MemoryWidget,
  TrafficDownWidget,
  TrafficUpWidget,
} from './widget-sparkline';

/** 所有 Dashboard Widget 的唯一标识符枚举 */
export enum WidgetId {
  TrafficDown = 'traffic-down',
  TrafficUp = 'traffic-up',
  Connections = 'connections',
  Memory = 'memory',
  ProxyShortcuts = 'proxy-shortcuts',
  CoreShortcuts = 'core-shortcuts',
}

/** Dashboard 网格项类型 — 在 DndGridItemType 基础上添加 type 字段 */
export type DashboardItem = DndGridItemType<string> & { type: WidgetId };

/** Widget 组件接收的 props */
export type WidgetComponentProps = {
  id: string;
  onCloseClick?: (id: string) => void;
};

/**
 * Widget 类型 → 组件渲染映射表
 * 每个 WidgetId 映射到对应的 React 组件
 */
export const RENDER_MAP: Record<
  WidgetId,
  (props: WidgetComponentProps) => ReactNode
> = {
  [WidgetId.TrafficDown]: TrafficDownWidget,
  [WidgetId.TrafficUp]: TrafficUpWidget,
  [WidgetId.Connections]: ConnectionsWidget,
  [WidgetId.Memory]: MemoryWidget,
  [WidgetId.ProxyShortcuts]: ProxyShortcutsWidget,
  [WidgetId.CoreShortcuts]: CoreShortcutsWidget,
};

/**
 * 默认布局（12 列网格）
 *
 * 布局说明：
 * - 第 0~1 行：4 个 Sparkline 卡片并排（TrafficDown, TrafficUp, Memory, Connections）
 * - 第 2 行起：ProxyShortcuts（3 列宽，3 行高）+ CoreShortcuts（4 列宽，2 行高）
 */
export const DEFAULT_ITEMS: DashboardItem[] = [
  {
    id: WidgetId.TrafficDown,
    type: WidgetId.TrafficDown,
    x: 0,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.TrafficUp,
    type: WidgetId.TrafficUp,
    x: 3,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.Memory,
    type: WidgetId.Memory,
    x: 6,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.Connections,
    type: WidgetId.Connections,
    x: 9,
    y: 0,
    w: 3,
    h: 2,
  },
  {
    id: WidgetId.ProxyShortcuts,
    type: WidgetId.ProxyShortcuts,
    x: 0,
    y: 2,
    w: 3,
    h: 3,
  },
  {
    id: WidgetId.CoreShortcuts,
    type: WidgetId.CoreShortcuts,
    x: 3,
    y: 2,
    w: 4,
    h: 2,
  },
];

/** 各 Widget 的最小尺寸限制 */
export const WIDGET_MIN_SIZE_MAP: Record<
  WidgetId,
  { minW: number; minH: number }
> = {
  [WidgetId.TrafficDown]: { minW: 2, minH: 2 },
  [WidgetId.TrafficUp]: { minW: 2, minH: 2 },
  [WidgetId.Connections]: { minW: 2, minH: 2 },
  [WidgetId.Memory]: { minW: 2, minH: 2 },
  [WidgetId.ProxyShortcuts]: { minW: 3, minH: 2 },
  [WidgetId.CoreShortcuts]: { minW: 4, minH: 2 },
};

/** 持久化存储的布局数据结构 */
export type LayoutStorage = Record<string, DashboardItem[]>;

/**
 * 针对不同网格尺寸的预设布局
 *
 * 键名为 `{cols}x{rows}` 格式，例如 '12x6' 表示 12 列 6 行的网格布局。
 * 当窗口尺寸变化时，WidgetRender 会查找最匹配的预设布局。
 *
 * 预设布局包括：4x5（最小）、8x6、12x6（默认）、16x6（宽屏）、20x6（超宽屏）
 */
export const DEFAULT_LAYOUTS: LayoutStorage = {
  // 4 列 5 行 — 极窄布局（手机/小窗口）
  '4x5': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 0,
      y: 2,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 2,
      y: 2,
      w: 2,
      h: 2,
    },
  ],
  // 8 列 6 行 — 窄屏布局
  '8x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 2,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 4,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 6,
      y: 0,
      w: 2,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 3,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  // 12 列 6 行 — 默认布局
  '12x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 3,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 6,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 9,
      y: 0,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 3,
      h: 2,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 3,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  // 16 列 6 行 — 宽屏布局
  '16x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 4,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 8,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 12,
      y: 0,
      w: 4,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 4,
      h: 3,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 4,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
  // 20 列 6 行 — 超宽屏布局
  '20x6': [
    {
      id: WidgetId.TrafficDown,
      type: WidgetId.TrafficDown,
      x: 0,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.TrafficUp,
      type: WidgetId.TrafficUp,
      x: 5,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.Memory,
      type: WidgetId.Memory,
      x: 10,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.Connections,
      type: WidgetId.Connections,
      x: 15,
      y: 0,
      w: 5,
      h: 2,
    },
    {
      id: WidgetId.ProxyShortcuts,
      type: WidgetId.ProxyShortcuts,
      x: 0,
      y: 2,
      w: 5,
      h: 3,
    },
    {
      id: WidgetId.CoreShortcuts,
      type: WidgetId.CoreShortcuts,
      x: 5,
      y: 2,
      w: 5,
      h: 2,
    },
  ],
};
