import { createFileRoute } from '@tanstack/react-router';
import * as m from '@/paraglide/messages';

export const Route = createFileRoute('/(legacy)/providers')({
  component: ProvidersPage,
});

function ProvidersPage() {
  return <div>{m.navbar_label_providers()}</div>;
}
