import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/connections')({
  component: Connections,
});

function Connections() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>connections</div>;
}
