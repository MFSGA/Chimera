import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/providers')({
  component: ProvidersPage,
});

function ProvidersPage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>providers</div>;
}
