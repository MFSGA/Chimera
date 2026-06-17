/**
 * Dashboard Widget 选择面板（底部抽屉）
 *
 * 迁移自 ref: `src/pages/(main)/main/dashboard/_modules/widget-sheet.tsx`
 *
 * 使用 vaul 的 Drawer 组件实现的底部抽屉面板，
 * 显示所有可用的 widget 预览列表。
 * 用户可以从这里拖拽 widget 添加到 Dashboard 主网格。
 *
 * 核心逻辑：
 * - sheetItems 使用 WIDGET_MIN_SIZE_MAP 计算每个 widget 的最小尺寸
 * - 使用 DndGrid（sourceOnly 模式）渲染可拖拽的 widget 预览
 * - 当用户拖拽 widget 到主网格时，通过 onSourceDrop 回调通知
 * - 拖拽开始时自动关闭抽屉（通过 onSourceDragStart）
 */

import CloseRounded from '~icons/material-symbols/close-rounded';
import { useMemo, useState } from 'react';
import { Drawer } from 'vaul';
import { Button } from '@/components/ui/button';
import { DndGrid, type GridSize } from '@/components/ui/dnd-grid';
import { ScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@chimera/ui';
import { RENDER_MAP, WIDGET_MIN_SIZE_MAP, WidgetId } from './consts';
import { useDashboardContext } from './provider';

export function WidgetSheet({
  onSourceDrop,
  onSourceDragStart,
}: {
  onSourceDrop: (id: WidgetId) => void;
  onSourceDragStart: () => void;
}) {
  const { openSheet, setOpenSheet } = useDashboardContext();

  const [gridSize, setGridSize] = useState<GridSize>();

  /**
   * 计算在 WidgetSheet 中展示所有可用 widget 所需的布局
   *
   * 按 WIDGET_MIN_SIZE_MAP 定义的最小尺寸排列，自动换行。
   * 如果 gridSize 尚未计算完成，返回空数组（等待首次 ResizeObserver 回调）。
   */
  const sheetItems = useMemo(() => {
    if (!gridSize) {
      return [];
    }

    const ids = Object.keys(RENDER_MAP) as WidgetId[];
    const result = [];
    let rowX = 0;
    let rowY = 0;
    let rowH = 0;

    for (const id of ids) {
      const { minW: w, minH: h } = WIDGET_MIN_SIZE_MAP[id];

      // 换行
      if (rowX + w > gridSize.cols) {
        rowY += rowH;
        rowX = 0;
        rowH = 0;
      }

      result.push({ id, x: rowX, y: rowY, w, h });
      rowX += w;
      rowH = Math.max(rowH, h);
    }

    return result;
  }, [gridSize]);

  return (
    <Drawer.Root open={openSheet} onOpenChange={setOpenSheet}>
      <Drawer.Portal>
        {/* 半透明背景遮罩 */}
        <Drawer.Overlay className="fixed inset-0 bg-black/30" />

        <Drawer.Content
          className={cn(
            'fixed inset-x-0 bottom-0 z-50 mx-auto max-w-96 min-w-96',
            'dark:bg-surface/30 bg-surface-variant/30 backdrop-blur-3xl',
            'h-full max-h-1/2 min-h-96 rounded-t-2xl',
            'dark:border-surface-variant/50 border-surface/50 border',
            'flex flex-col',
          )}
          aria-describedby={undefined}
        >
          {/* 头部标题 + 关闭按钮 */}
          <div className="flex items-center justify-between gap-4 p-4">
            <Drawer.Title className="text-lg font-semibold">
              添加小组件
            </Drawer.Title>

            <Drawer.Close asChild>
              <Button variant="raised" className="size-8" icon>
                <CloseRounded className="size-4" />
              </Button>
            </Drawer.Close>
          </div>

          {/* Widget 列表滚动区域 */}
          <ScrollArea
            className={cn(
              'min-h-0 flex-1',
              '[&_[data-slot=scroll-area-viewport]>div]:block!',
              '[&_[data-slot=scroll-area-viewport]>div]:h-full',
            )}
          >
            <div className="flex h-full w-full flex-col px-4">
              <DndGrid
                gridId="sheet"
                className="min-h-0 flex-1"
                items={sheetItems}
                minCellSize={64}
                gap={16}
                disabled={false}
                sourceOnly
                dragIdPrefix="sheet:"
                onSourceDrop={(id) => onSourceDrop(id as WidgetId)}
                onSourceDragStart={onSourceDragStart}
                onSizeChange={(size) => setGridSize(size)}
              >
                {(item) => {
                  const WidgetComponent = RENDER_MAP[item.id as WidgetId];

                  return (
                    <WidgetComponent id={item.id} onCloseClick={() => {}} />
                  );
                }}
              </DndGrid>
            </div>
          </ScrollArea>
        </Drawer.Content>
      </Drawer.Portal>
    </Drawer.Root>
  );
}
