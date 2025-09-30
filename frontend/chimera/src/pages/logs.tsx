import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/logs')({
  component: LogPage,
});

function LogPage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>log</div>;
}
