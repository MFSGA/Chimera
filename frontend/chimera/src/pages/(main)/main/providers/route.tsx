import { cn } from '@chimera/ui';
import { createFileRoute, useLocation } from '@tanstack/react-router';
import { motion } from 'framer-motion';
import { AnimatedOutletPreset } from '@/components/router/animated-outlet';
import { AppContentScrollArea } from '@/components/ui/scroll-area';
import { Sidebar, SidebarContent } from '@/components/ui/sidebar';
import useIsMobile from '@/hooks/use-is-moblie';

export const Route = createFileRoute('/(main)/main/providers')({
  component: ProvidersLayout,
});

const SidebarNavigate = () => {
  return null;
};

function ProvidersLayout() {
  const { pathname } = useLocation();
  const isCurrent = pathname === Route.fullPath;
  const isMobile = useIsMobile();

  return (
    <Sidebar data-slot="providers-container">
      {!isCurrent && !isMobile && (
        <motion.div
          animate={{ opacity: 1, x: 0 }}
          initial={{ opacity: 0, x: -24 }}
          transition={{
            duration: 0.28,
            ease: [0.22, 1, 0.36, 1],
          }}
        >
          <SidebarContent
            className="bg-surface-variant/10 [&>div>div]:block!"
            data-slot="providers-sidebar-scroll-area"
          >
            <SidebarNavigate />
          </SidebarContent>
        </motion.div>
      )}

      <AppContentScrollArea
        className={cn(
          'group/providers-content flex-[3_1_auto]',
          'overflow-clip',
        )}
        data-slot="providers-content-scroll-area"
      >
        <div
          className={cn('container mx-auto w-full max-w-7xl', 'min-h-full')}
          data-slot="providers-content"
        >
          <AnimatedOutletPreset />
        </div>
      </AppContentScrollArea>
    </Sidebar>
  );
}
