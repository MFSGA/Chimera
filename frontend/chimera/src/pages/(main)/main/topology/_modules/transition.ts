import { useReducedMotion } from 'motion/react';

/** Use the standard easing unless the system asks for reduced motion. */
export function usePageTransition() {
  const reducedMotion = useReducedMotion() ?? false;

  return {
    reducedMotion,
    transition: {
      duration: reducedMotion ? 0 : 0.32,
      ease: [0.2, 0, 0, 1] as const,
    },
  };
}
