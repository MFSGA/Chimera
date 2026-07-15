import { cn } from '@chimera/ui';
import { createFileRoute, Outlet } from '@tanstack/react-router';

export const Route = createFileRoute('/(editor)/editor')({
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <div
      className={cn('bg-background/30 flex h-dvh flex-col')}
      data-slot="editor-container"
      onContextMenu={(event) => event.preventDefault()}
    >
      <Outlet />
    </div>
  );
}
