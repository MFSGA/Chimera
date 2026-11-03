import { ClashConnectionDetailsFreezeBoundary } from '@chimera/interface';
import {
  Outlet,
  RouterContextProvider,
  useMatch,
  useMatches,
  useRouter,
  useRouterState,
} from '@tanstack/react-router';
import {
  AnimatePresence,
  motion,
  PresenceContext,
  usePresence,
  Variants,
} from 'motion/react';
import { ComponentProps, useRef } from 'react';

type TransitionDirection = 1 | -1;

export function getRouteRenderKey(matchId: string | undefined) {
  if (!matchId) return undefined;

  let hash = 2166136261;
  for (let index = 0; index < matchId.length; index += 1) {
    hash = Math.imul(hash ^ matchId.charCodeAt(index), 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, '0');
}

// Drive a whole CSS transform on the compositor, as in ref v2.0.1. A
// settled page must drop the transform to preserve fixed descendants.
const directionalSlideVariants = {
  forward: {
    initial: {
      transform: 'translateX(30%)',
      opacity: 0,
    },
    visible: {
      transform: 'translateX(0%)',
      opacity: 1,
      transitionEnd: { transform: 'none' },
    },
    hidden: {
      transform: 'translateX(-30%)',
      opacity: 0,
    },
  },
  backward: {
    initial: {
      transform: 'translateX(-30%)',
      opacity: 0,
    },
    visible: {
      transform: 'translateX(0%)',
      opacity: 1,
      transitionEnd: { transform: 'none' },
    },
    hidden: {
      transform: 'translateX(30%)',
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
  onAnimationComplete,
  ...props
}: ComponentProps<typeof motion.div>) {
  const [isPresent, safeToRemove] = usePresence();

  const matches = useMatches();
  const prevMatches = useRef(matches);

  const router = useRouter();
  // Match ref: freeze the state that belonged to the leaving page, not the
  // already-committed destination route (its location and match IDs differ).
  const prevState = useRef(router.state);

  // Frozen router for the exit animation, created once when isPresent becomes false
  const frozenRouterRef = useRef<typeof router | null>(null);

  let renderedRouter = router;

  if (isPresent) {
    prevMatches.current = matches;
    prevState.current = router.state;
    frozenRouterRef.current = null;
  } else {
    if (!frozenRouterRef.current) {
      // Keep the exact leaving matches and their IDs. Rewriting them to the
      // incoming route IDs can render the old card under the new route state.
      const frozenMatches = prevMatches.current;
      const { resolvedLocation, location } = prevState.current;
      const frozenState = {
        ...prevState.current,
        location: resolvedLocation ?? location,
        matches: frozenMatches,
      };

      // Helper: create a static frozen store for the exit animation.
      // useStore (@tanstack/react-store) needs .get() and .subscribe(); returning a
      // no-op unsubscribe means the frozen store never triggers re-renders.
      // Use function syntax to avoid <T> being parsed as JSX in .tsx files.
      function frozenStore<T>(value: T) {
        return {
          state: value,
          get: () => value,
          subscribe: (_: () => void) => ({ unsubscribe: () => {} }),
        };
      }

      // router.stores is a v1.168+ internal API not yet reflected in all type declarations
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      const routerStores = (router as any).stores;

      // Adapt ref's frozen route-ID stores to the installed router's
      // matchStores/matchesId API without changing the leaving route identity.
      const fakeMatchStores = new Map(routerStores.matchStores);
      const routeIdToFrozenStore = new Map<
        string,
        ReturnType<typeof frozenStore>
      >();
      frozenMatches.forEach((m) => {
        const store = frozenStore(m);
        // The real matchStores entries carry a `routeId` property alongside the
        // store interface — mirror that so any code that reads it keeps working.
        (store as typeof store & { routeId?: string }).routeId = m.routeId;
        fakeMatchStores.set(m.id, store);
        if (m.routeId) routeIdToFrozenStore.set(m.routeId, store);
      });

      // Freeze all route selectors together so the leaving Outlet cannot
      // pick up the destination's matches during its exit animation.
      const fakeStores = Object.create(routerStores);
      Object.defineProperty(fakeStores, 'matches', {
        value: frozenStore(frozenMatches),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'matchesId', {
        value: frozenStore(frozenMatches.map((m) => m.id)),
        configurable: true,
      });
      // Match ref's snapshot of route navigation state as well as match data.
      // Otherwise exiting Outlet descendants still observe the incoming URL.
      Object.defineProperty(fakeStores, 'location', {
        value: frozenStore(frozenState.location),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'resolvedLocation', {
        value: frozenStore(frozenState.resolvedLocation),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'status', {
        value: frozenStore(frozenState.status),
        configurable: true,
      });
      Object.defineProperty(fakeStores, 'matchStores', {
        value: fakeMatchStores,
        configurable: true,
      });
      // getRouteMatchStore is called by useMatch({ from }) inside route components
      Object.defineProperty(fakeStores, 'getRouteMatchStore', {
        value: (routeId: string) =>
          routeIdToFrozenStore.get(routeId) ?? frozenStore(undefined),
        configurable: true,
      });

      // Create a fake router that delegates everything to the real router except stores/state
      const fakeRouter = Object.create(router);
      Object.defineProperty(fakeRouter, 'stores', {
        value: fakeStores,
        configurable: true,
      });
      Object.defineProperty(fakeRouter, 'state', {
        get: () => frozenState,
        configurable: true,
      });

      frozenRouterRef.current = fakeRouter;
    }

    // force type safety
    renderedRouter = frozenRouterRef.current!;
  }

  return (
    <motion.div
      ref={ref}
      {...props}
      onAnimationComplete={(definition) => {
        // An outgoing route may keep nested Motion descendants mounted under
        // its frozen router. Release its own presence registration once its
        // actual slide-out completes, rather than leaving a fully hidden old
        // route (and its duplicate profile cards) mounted indefinitely.
        if (!isPresent && definition === 'hidden') {
          safeToRemove?.();
        }
        onAnimationComplete?.(definition);
      }}
      data-route-present={isPresent ? 'true' : 'false'}
    >
      <RouterContextProvider router={renderedRouter}>
        {/* The outer page owns the route slide. Do not register its nested
            motion cards with that same exit barrier: a frozen leaving route
            can keep an already-hidden page alive after the slide finishes.
            Inner AnimatePresence instances still manage their own children. */}
        <PresenceContext value={null}>
          <ClashConnectionDetailsFreezeBoundary frozen={!isPresent}>
            <Outlet />
          </ClashConnectionDetailsFreezeBoundary>
        </PresenceContext>
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
        data-route-id={nextMatch?.routeId}
        data-route-key={getRouteRenderKey(nextMatch?.id)}
      />
    </AnimatePresence>
  );
}
