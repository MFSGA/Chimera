import { createFileRoute, redirect } from '@tanstack/react-router';
import { FileRouteTypes } from '@/routeTree.gen';

const fallbackRoute = '/dashboard' satisfies FileRouteTypes['to'];

const normalizeMemorizedRoute = (value: unknown): FileRouteTypes['to'] => {
  if (value === '/main/') {
    return '/main';
  }

  if (typeof value === 'string' && value !== '/') {
    return value as FileRouteTypes['to'];
  }

  return fallbackRoute;
};

const getMemorizedRoute = () => {
  try {
    return normalizeMemorizedRoute(
      JSON.parse(localStorage.getItem('memorizedRoutePathAtom') ?? 'null'),
    );
  } catch {
    return fallbackRoute;
  }
};

export const Route = createFileRoute('/(legacy)/')({
  beforeLoad: () => {
    throw redirect({
      to: getMemorizedRoute(),
    });
  },
  component: IndexPage,
});

function IndexPage() {
  return null;
}
