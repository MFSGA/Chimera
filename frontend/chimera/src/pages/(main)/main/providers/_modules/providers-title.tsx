/**
 * Providers 页面标题组件
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/_modules/providers-title.tsx`
 *
 * 功能：
 * - 提供返回按钮，导航到 /main/providers
 * - 滚动时显示/隐藏大标题（sticky 效果）
 * - 使用 framer-motion 实现标题过渡动画
 */

import { cn } from '@chimera/ui';
import { Link } from '@tanstack/react-router';
import { AnimatePresence, motion } from 'framer-motion';
import { useId, type ComponentProps } from 'react';
import { Button } from '@/components/ui/button';
import { useScrollArea } from '@/components/ui/scroll-area';

/** 返回按钮 SVG */
function BackArrowIcon() {
  return (
    <svg
      className="size-4"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
    >
      <path d="M20 12H4M10 18l-6-6 6-6" />
    </svg>
  );
}

const BackButton = () => {
  return (
    <Button
      icon
      variant="raised"
      className="flex items-center justify-center"
      asChild
    >
      <Link to="/main/providers">
        <BackArrowIcon />
      </Link>
    </Button>
  );
};

const Title = (props: ComponentProps<typeof motion.div>) => {
  return (
    <motion.div
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

export function ProvidersTitle({
  className,
  children,
  ...props
}: ComponentProps<'div'>) {
  const { offset } = useScrollArea();

  const id = useId();

  const showTopTitle = offset.top > 40;

  return (
    <>
      <div
        className={cn(
          'group sticky top-0 z-10',
          'bg-mixed-background',
          'flex items-center gap-4',
          'h-16 px-4',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot="providers-title"
        {...props}
      >
        <BackButton />

        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="providers-title-top"
              layoutId={id}
              className="text-xl font-bold"
            >
              {children}
            </Title>
          )}
        </AnimatePresence>
      </div>

      <div
        className="group flex h-24 px-6 pt-10 pb-4"
        data-slot="providers-title"
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title
              key="providers-title-main"
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
