import { Button } from '@chimera/ui/button';
import { useScrollArea } from '@chimera/ui/scroll-area';
import { cn } from '@chimera/utils';
import { Link, type LinkProps } from '@tanstack/react-router';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useId, type ComponentProps, type ReactNode } from 'react';
import useIsMobile from '@/hooks/use-is-moblie';
import * as m from '@/paraglide/messages';
import { useCrumbParent, useLeafCrumb } from './router/route-crumbs';

export const BackButton = ({
  to,
  params,
  label,
  search,
  className,
  variant = 'raised',
  ...props
}: {
  to?: LinkProps['to'];
  params?: LinkProps['params'];
  /** Accessible name for this icon-only link. */
  label?: string;
  search?: LinkProps['search'];
} & Omit<ComponentProps<typeof Button>, 'asChild' | 'children' | 'icon'>) => {
  const parent = useCrumbParent();
  const isMobile = useIsMobile();
  const target = to ?? parent?.to;

  if (!target || (to === undefined && parent?.hub && !isMobile)) {
    return null;
  }

  const targetParams = to ? params : parent?.params;
  const accessibleLabel =
    label ??
    (parent
      ? m.navigation_return_to({ page: parent.title })
      : m.profile_back());

  return (
    <Button
      icon
      variant={variant}
      className={cn(
        'flex items-center justify-center',
        to !== undefined && 'md:hidden',
        className,
      )}
      asChild
      {...props}
    >
      <Link
        to={target as LinkProps['to']}
        params={targetParams as LinkProps['params']}
        search={search}
        aria-label={accessibleLabel}
      >
        <ArrowBackIosNewRounded className="size-4" />
      </Link>
    </Button>
  );
};

const Title = (props: ComponentProps<typeof motion.p>) => (
  <motion.p
    layout
    layoutCrossfade={false}
    transition={{
      layout: { duration: 0.5, ease: [0.32, 0.72, 0, 1] },
      opacity: { duration: 0.16 },
    }}
    {...props}
  />
);

export function PageTitle({
  title,
  actions,
  className,
  legacySlot,
  ...props
}: ComponentProps<'div'> & {
  /** Overrides the current route crumb's localized title. */
  title?: ReactNode;
  actions?: ReactNode;
  /** Temporary E2E selector compatibility until the settings tests migrate. */
  legacySlot?: string;
}) {
  const { isPastHeader } = useScrollArea();
  const leaf = useLeafCrumb();
  const id = useId();
  const showTopTitle = isPastHeader;
  const shownTitle =
    title ?? (leaf ? leaf.crumb.title({ params: leaf.params }) : null);
  const slot = legacySlot ?? 'page-title';

  return (
    <>
      <div
        className={cn(
          'group sticky top-0 z-10',
          'bg-mixed-background',
          'flex items-center gap-4',
          'h-16 px-4 md:px-6',
          className,
        )}
        data-show-title={showTopTitle}
        data-slot={slot}
        {...props}
      >
        <BackButton />
        <AnimatePresence initial={false}>
          {showTopTitle && (
            <Title
              key="page-title-top"
              layoutId={id}
              className="text-xl font-bold"
            >
              {shownTitle}
            </Title>
          )}
        </AnimatePresence>
        {actions && (
          <div className="ml-auto flex shrink-0 items-center gap-1">
            {actions}
          </div>
        )}
      </div>

      <div
        className="group relative z-20 flex h-24 px-6 pt-10 pb-4"
        data-slot={slot}
        data-show-title={!showTopTitle}
      >
        <AnimatePresence initial={false}>
          {!showTopTitle && (
            <Title
              key="page-title-main"
              layoutId={id}
              className="text-3xl font-bold"
            >
              {shownTitle}
            </Title>
          )}
        </AnimatePresence>
      </div>
    </>
  );
}
