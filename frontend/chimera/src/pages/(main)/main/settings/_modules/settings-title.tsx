/**
 * 设置页面粘性标题组件
 *
 * 迁移自 ref: `src/pages/(main)/main/settings/_modules/settings-title.tsx`
 *
 * 职责：
 * - 在设置子页面顶部显示粘性标题
 * - 滚动距离 < 40px 时：在页面内显示大标题（text-3xl）
 * - 滚动距离 >= 40px 时：在 sticky header 中显示小标题（text-xl）
 * - 使用 framer-motion 的 AnimatePresence + layout animation 实现平滑过渡
 * - 在移动端（md 以下）显示返回按钮，跳转至 /main/settings
 *
 * 实现说明：
 * - 使用 useScrollArea().offset.top 获取滚动距离（与 ref 一致）
 * - 两个标题区域使用共享的 useId 作为 layoutId，确保 framer-motion 识别为同一元素
 * - 动画时长和缓动曲线匹配 ref: 0.5s, [0.32, 0.72, 0, 1]
 */

import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import { AnimatePresence, motion } from 'framer-motion';
import { useId, type ComponentProps } from 'react';
import { Button } from '@/components/ui/button';
import { useScrollArea } from '@/components/ui/scroll-area';
import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';

/**
 * 返回按钮 — 仅在移动端 (md 以下) 显示
 * 点击跳转至设置主页 /main/settings
 */
const BackButton = () => {
  return (
    <Button
      icon
      variant="raised"
      className="flex items-center justify-center md:hidden"
      asChild
    >
      <Link to="/main/settings">
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  );
};

/**
 * Animated Title — 使用 framer-motion 实现 layout 动画的标题文本
 * layoutId 与 useId 绑定，确保两个标题区域共享同一动画标识
 */
const Title = (props: ComponentProps<typeof motion.p>) => {
  return (
    <motion.p
      layout
      transition={{
        layout: {
          duration: 0.5,
          ease: [0.32, 0.72, 0, 1],
        },
        opacity: {
          duration: 0.16,
        },
      }}
      {...props}
    />
  );
};

/**
 * SettingsTitle — 设置页面粘性标题
 *
 * @param children - 标题文本内容
 *
 * 布局结构：
 * 1. sticky header (h-16)：始终固定在顶部，滚动 >= 40px 时显示小标题
 * 2. 页面内标题 (h-24)：滚动 < 40px 时显示大标题
 *
 * 使用 data-show-title 属性记录当前显示状态，方便外部样式选择
 */
export function SettingsTitle({
  className,
  children,
  ...props
}: ComponentProps<'div'>) {
  const { offset } = useScrollArea();

  const id = useId();

  const showTopTitle = offset.top > 40;

  return (
    <>
      {/* sticky header — 滚动 >= 40px 时显示小标题 */}
      <div
        className={cn(
          'group sticky top-0 z-10',
          'bg-mixed-background',
          'flex items-center gap-4',
          'h-16 px-4 md:px-6',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot="settings-title"
        {...props}
      >
        <BackButton />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="settings-title-top"
              layoutId={id}
              className="text-xl font-bold"
            >
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>

      {/* 页面内标题 — 滚动 < 40px 时显示大标题 */}
      <div
        className="group flex h-24 px-6 pt-10 pb-4"
        data-slot="settings-title"
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title
              key="settings-title-main"
              layoutId={id}
              className="text-3xl font-bold"
            >
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>
    </>
  );
}
