import { createFileRoute, Link } from '@tanstack/react-router';

export const Route = createFileRoute('/(main)/main/providers/')({
  component: ProvidersPage,
});

function ProvidersPage() {
  return <div className="flex flex-col gap-4 p-4">ProvidersPage</div>;
}
