import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';

export const Route = createFileRoute('/(main)/main/providers')({
  component: ProvidersLayout,
});

function ProvidersLayout() {
  return (
    <div
      className={cn('container mx-auto w-full max-w-7xl', 'min-h-full')}
      data-slot="providers-content"
    >
      <AnimatedOutletPreset />
    </div>
  );
}
