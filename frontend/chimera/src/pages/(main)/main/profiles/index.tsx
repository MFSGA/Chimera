import { AppContentScrollArea } from '@chimera/ui/scroll-area';
import { cn } from '@chimera/utils';
import { createFileRoute } from '@tanstack/react-router';
import useIsMobile from '@/hooks/use-is-moblie';
import ProfilesNavigate from './_modules/profiles-navigate';

export const Route = createFileRoute('/(main)/main/profiles/')({
  component: RouteComponent,
});

function RouteComponent() {
  const isMobile = useIsMobile();

  if (!isMobile) return null;

  return (
    <AppContentScrollArea
      className={cn(
        // The mobile route transition overlaps the desktop sidebar, so keep it opaque.
        'dark:bg-mixed-background bg-[color-mix(in_srgb,var(--color-surface-variant)_50%,var(--color-surface))]',
        'z-50 w-full flex-1 [&>div>div]:block!',
      )}
      data-slot="profiles-sidebar-scroll-area"
    >
      <ProfilesNavigate />
    </AppContentScrollArea>
  );
}
