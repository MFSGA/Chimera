import {
  Outlet,
  RouterContextProvider,
  useMatch,
  useMatches,
  useRouter,
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
  const matches = useMatches();
  const prevMatches = useRef(matches);
  const router = useRouter();
  const frozenRouterRef = useRef<typeof router | null>(null);

  let renderedRouter = router;

  if (isPresent) {
    prevMatches.current = matches;
    frozenRouterRef.current = null;
  } else {
    if (!frozenRouterRef.current) {
      const patchedMatches = [
        ...matches.map((match, index) => ({
          ...(prevMatches.current[index] || match),
          id: match.id,
        })),
        ...prevMatches.current.slice(matches.length),
      ];
      const patchedState = {
        ...router.state,
        matches: patchedMatches,
      };

      function frozenStore<T>(value: T) {
        return {
          state: value,
          get: () => value,
          subscribe: (_: () => void) => ({ unsubscribe: () => {} }),
        };
      }

      const routerStores = (
        router as typeof router & {
          stores: {
            matchStores: Map<string, unknown>;
            getRouteMatchStore: (routeId: string) => unknown;
          };
        }
      ).stores;

      const fakeMatchStores = new Map(routerStores.matchStores);
      const routeIdToFrozenStore = new Map<
        string,
        ReturnType<typeof frozenStore>
      >();

      patchedMatches.forEach((match) => {
        const store = frozenStore(match);
        (store as typeof store & { routeId?: string }).routeId = match.routeId;
        fakeMatchStores.set(match.id, store);
        if (match.routeId) {
          routeIdToFrozenStore.set(match.routeId, store);
        }
      });

      const fakeStores = Object.create(routerStores);

      Object.defineProperty(fakeStores, 'matches', {
        value: frozenStore(patchedMatches),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'matchesId', {
        value: frozenStore(patchedMatches.map((match) => match.id)),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'matchStores', {
        value: fakeMatchStores,
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'getRouteMatchStore', {
        value: (routeId: string) =>
          routeIdToFrozenStore.get(routeId) ?? frozenStore(undefined),
        configurable: true,
      });

      const fakeRouter = Object.create(router);

      // Keep exiting routes subscribed to their previous match snapshot.
      Object.defineProperty(fakeRouter, 'stores', {
        value: fakeStores,
        configurable: true,
      });
      Object.defineProperty(fakeRouter, 'state', {
        get: () => patchedState,
        configurable: true,
      });

      frozenRouterRef.current = fakeRouter;
    }

    renderedRouter = frozenRouterRef.current!;
  }

  return (
    <motion.div ref={ref} {...props}>
      <RouterContextProvider router={renderedRouter}>
        <Outlet />
      </RouterContextProvider>
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
