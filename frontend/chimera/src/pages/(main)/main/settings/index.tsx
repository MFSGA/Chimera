import { cn } from '@chimera/ui';
import { AppContentScrollArea } from '@chimera/ui/scroll-area';
import { createFileRoute } from '@tanstack/react-router';
import useIsMobile from '@/hooks/use-is-moblie';
import SettingsNavigate from './_modules/settings-navigate';

export const Route = createFileRoute('/(main)/main/settings/')({
  component: RouteComponent,
});

function RouteComponent() {
  const isMobile = useIsMobile();

  if (!isMobile) {
    return null;
  }

  return (
    <AppContentScrollArea
      className={cn('bg-surface z-50 w-full flex-1 [&>div>div]:block!')}
      data-slot="settings-sidebar-scroll-area"
    >
      <SettingsNavigate />
    </AppContentScrollArea>
  );
}
