import { useSettings } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { PropsWithChildren } from 'react';
import packageJson from '@root/package.json';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import Header from './_modules/-header';
import Navbar from './_modules/-navbar';

export const Route = createFileRoute('/(main)')({
  component: RouteComponent,
});

const QueryLoaderProvider = ({ children }: PropsWithChildren) => {
  const {
    query: { isLoading },
  } = useSettings();

  return isLoading ? null : children;
};

const AppContent = () => {
  return (
    <AnimatedOutletPreset
      className={cn(
        'h-[calc(100vh-40px-64px)] overflow-hidden',
        'sm:h-[calc(100vh-40px-48px)]',
      )}
      data-slot="app-content"
    />
  );
};

function RouteComponent() {
  return (
    <div
      className={cn('flex max-h-dvh min-h-dvh flex-col', 'bg-mixed-background')}
      data-slot="app-root"
      data-app-version={packageJson.version}
    >
      <Header />

      <div
        className="flex flex-1 flex-col sm:flex-col-reverse"
        data-slot="app-content-container"
      >
        <AppContent />

        <Navbar />
      </div>
    </div>
  );
}
