import { createFileRoute } from '@tanstack/react-router';

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
});

function ProxyPage() {
  // todo: use i18n
  // const { t } = useTranslation()

  return <div>proxies</div>;
}
