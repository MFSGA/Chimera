import {
  Outlet,
  useMatch,
  useMatches,
  useRouterState,
} from '@tanstack/react-router';
import { AnimatePresence, motion, useIsPresent, Variants } from 'framer-motion';
import { ComponentProps, useRef } from 'react';

type TransitionDirection = 1 | -1;

const directionalSlideVariants = {
  forward: {
    initial: {
      translateX: '30%',
      opacity: 0,
    },
    visible: {
      translateX: '0%',
      opacity: 1,
    },
    hidden: {
      translateX: '-30%',
      opacity: 0,
    },
  },
  backward: {
    initial: {
      translateX: '-30%',
      opacity: 0,
    },
    visible: {
      translateX: '0%',
      opacity: 1,
    },
    hidden: {
      translateX: '30%',
      opacity: 0,
    },
  },
} satisfies Record<'forward' | 'backward', Variants>;

function getDirectionalVariant(direction: TransitionDirection) {
  return direction === 1
    ? directionalSlideVariants.forward
    : directionalSlideVariants.backward;
}

export function AnimatedOutlet({
  ref,
  ...props
}: ComponentProps<typeof motion.div>) {
  const isPresent = useIsPresent();

  return (
    <motion.div ref={ref} {...props}>
      {isPresent && <Outlet />}
    </motion.div>
  );
}

export function AnimatedOutletPreset(props: ComponentProps<typeof motion.div>) {
  const matches = useMatches();
  const match = useMatch({ strict: false });
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  });
  const nextMatchIndex = matches.findIndex((d) => d.id === match.id) + 1;
  const nextMatch = matches[nextMatchIndex];

  const id = nextMatch ? nextMatch.id : '';
  const prevPathRef = useRef(pathname);
  const directionRef = useRef<TransitionDirection>(1);

  if (prevPathRef.current !== pathname) {
    const prevPath = prevPathRef.current;
    const nextPath = pathname;

    if (nextPath.startsWith(`${prevPath}/`)) {
      directionRef.current = 1;
    } else if (prevPath.startsWith(`${nextPath}/`)) {
      directionRef.current = -1;
    } else {
      // Non-ancestor navigation (including sibling routes) uses forward animation.
      directionRef.current = 1;
    }

    prevPathRef.current = pathname;
  }

  const direction = directionRef.current;
  const selectedVariants = getDirectionalVariant(direction);

  return (
    <AnimatePresence mode="popLayout" initial={false} custom={direction}>
      <AnimatedOutlet
        key={id}
        custom={direction}
        layout="position"
        initial="initial"
        animate="visible"
        exit="hidden"
        variants={{
          initial: (customDirection: TransitionDirection) =>
            getDirectionalVariant(customDirection).initial,
          visible: selectedVariants.visible,
          hidden: (customDirection: TransitionDirection) =>
            getDirectionalVariant(customDirection).hidden,
        }}
        transition={{
          type: 'spring',
          bounce: 0.1,
          duration: 0.35,
        }}
        {...props}
      />
    </AnimatePresence>
  );
}
