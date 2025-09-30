import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/profiles')({
  component: ProfilePage,
});

function ProfilePage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>profiles</div>;
}
