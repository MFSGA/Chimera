import { useBreakpoint } from '@chimera/ui';

export default function useIsMobile() {
  const breakpoint = useBreakpoint();

  return breakpoint === 'sm' || breakpoint === 'xs';
}

export function useIsMobileOrTablet() {
  const breakpoint = useBreakpoint();

  return breakpoint === 'sm' || breakpoint === 'xs' || breakpoint === 'md';
}
