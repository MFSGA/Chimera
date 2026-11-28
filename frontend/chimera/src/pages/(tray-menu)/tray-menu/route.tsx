import { cn } from '@chimera/utils';
import { createFileRoute } from '@tanstack/react-router';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { m } from '@/paraglide/messages';

export const Route = createFileRoute('/(tray-menu)/tray-menu')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.tray_menu_open_dashboard() } },
});

function RouteComponent() {
  return (
    <div
      className={cn('h-dvh w-dvw overflow-hidden', 'bg-background')}
      data-slot="tray-menu-container"
      onContextMenu={(e) => e.preventDefault()}
    >
      <AnimatedOutletPreset />
    </div>
  );
}
