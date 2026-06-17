/**
 * 设置页面卡片组件
 *
 * 迁移自 ref: `src/pages/(main)/main/settings/_modules/settings-card.tsx`
 *
 * 职责：
 * - 提供设置页面的卡片布局组件（Card、CardHeader、CardContent、CardFooter）
 * - 提供设置项容器和标签组件（ItemContainer、ItemLabel、ItemLabelText、ItemLabelDescription）
 * - 标签/描述分组组件（SettingsLabel、SettingsGroup）
 *
 * 适配说明：
 * - Chimera 无 Card 组件封装，使用 div + Tailwind 类实现等效样式
 * - 移除了 ref 对 @nyanpasu/components Card 的依赖
 * - 移除了 ref 的 AnimatedItem（Chimera 无对应组件）
 * - 圆角、内边距、颜色等视觉参数尽量匹配 ref
 * - 使用 data-slot 属性标识组件角色，方便 QA 定位
 */

import { type ComponentProps } from 'react';
import { cn } from '@chimera/ui';

/**
 * SettingsLabel — 设置区域标签
 * 用于分隔不同设置区块的文本标签
 */
export function SettingsLabel({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'text-on-primary-container px-3 py-3 text-sm',
        className,
      )}
      data-slot="settings-label"
      {...props}
    />
  );
}

/**
 * SettingsGroup — 设置项分组容器
 * 将多个设置卡片或项组合在一起，自动调整相邻元素的圆角
 * - 唯一子元素：全部圆角
 * - 首个元素：上圆角，下平角
 * - 末个元素：下圆角，上平角
 * - 中间元素：无圆角
 */
export function SettingsGroup({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex flex-col gap-1 *:transition-[border-radius]',
        '[&>*:first-child:not(:only-child)]:rounded-b-sm',
        '[&>*:last-child:not(:only-child)]:rounded-t-sm',
        '[&>*:not(:first-child):not(:last-child)]:rounded-sm',
        className,
      )}
      data-slot="settings-group"
      {...props}
    />
  );
}

/**
 * SettingsCard — 设置卡片
 * 使用 Card 语义的外观容器，包裹设置项
 */
export function SettingsCard({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        // ref 的 Card 组件样式映射：圆角 + 背景色 + 投影
        'rounded-xl bg-surface-variant/20 shadow-sm',
        className,
      )}
      data-slot="settings-card"
      {...props}
    />
  );
}

/**
 * SettingsCardHeader — 卡片头部
 * 通常包含标题或说明文字
 */
export function SettingsCardHeader({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('px-5 pt-6', className)}
      data-slot="settings-card-header"
      {...props}
    />
  );
}

/**
 * SettingsCardFooter — 卡片底部
 * 通常包含操作按钮或状态指示
 */
export function SettingsCardFooter({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('px-3 pb-3', className)}
      data-slot="settings-card-footer"
      {...props}
    />
  );
}

/**
 * SettingsCardContent — 卡片内容区域
 * 包含设置项列表
 */
export function SettingsCardContent({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex flex-col gap-6 px-5 py-6', className)}
      data-slot="settings-card-content"
      {...props}
    />
  );
}

/**
 * ItemContainer — 设置项行容器
 * 使用 flex 布局将标签和操作控件分列两侧
 */
export function ItemContainer({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn(
        'flex items-center justify-between gap-4',
        className,
      )}
      data-slot="settings-card-content-item-container"
      {...props}
    />
  );
}

/**
 * ItemLabel — 设置项标签容器
 * 包含标题文本和描述文本，垂直排列
 */
export function ItemLabel({
  className,
  ...props
}: ComponentProps<'div'>) {
  return (
    <div
      className={cn('flex flex-col gap-0.5', className)}
      data-slot="settings-card-content-item-label"
      {...props}
    />
  );
}

/**
 * ItemLabelText — 设置项标题文本
 * 粗体，用于标识设置项名称
 */
export function ItemLabelText({
  className,
  ...props
}: ComponentProps<'p'>) {
  return (
    <p
      className={cn('text-base font-medium', className)}
      data-slot="settings-card-content-item-label-text"
      {...props}
    />
  );
}

/**
 * ItemLabelDescription — 设置项描述文本
 * 灰度小字，用于说明设置项的功能
 */
export function ItemLabelDescription({
  className,
  ...props
}: ComponentProps<'p'>) {
  return (
    <p
      className={cn(
        'text-on-surface-variant text-sm',
        className,
      )}
      data-slot="settings-card-content-item-label-description"
      {...props}
    />
  );
}
