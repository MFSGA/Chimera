import { useProxyMode } from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
});

function ProxyPage() {
  const { t } = useTranslation();

  const { value: proxyMode, upsert } = useProxyMode();

  return (
    <SidePage>
      <div>hello {proxyMode.direct ? 'Direct Mode' : 'Proxy Mode'}</div>
    </SidePage>
  );
}
