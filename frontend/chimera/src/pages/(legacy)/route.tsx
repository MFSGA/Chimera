import { createFileRoute, Outlet } from '@tanstack/react-router';

export const Route = createFileRoute('/(legacy)')({
  component: LegacyLayout,
});

function LegacyLayout() {
  return <Outlet />;
}
