import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/rules')({
  component: RulesPage,
});

function RulesPage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>rules</div>;
}
