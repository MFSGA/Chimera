import { useClashProxies, useProxyMode } from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
});

function ProxyPage() {
  const { t } = useTranslation();

  const { value: proxyMode, upsert } = useProxyMode();

  const { data } = useClashProxies();

  const hasProxies = Boolean(data?.groups.length);

  console.log(data);

  return (
    <SidePage>
      {!proxyMode.direct ? (
        hasProxies ? (
          <>
            <div>
              {/* <ContentDisplay className="absolute" message={t('Direct Mode')} /> */}
              hasProxies
              {/* <NodeList
              ref={nodeListRef}
              scrollRef={rightViewportRef as RefObject<HTMLElement>}
            />

            <DelayButton onClick={handleDelayClick} /> */}
            </div>
          </>
        ) : (
          <div>
            {/* <ContentDisplay className="absolute" message={t('Direct Mode')} /> */}
            no hasProxies
          </div>
        )
      ) : (
        <div>
          {/* <ContentDisplay className="absolute" message={t('Direct Mode')} /> */}
          direct
        </div>
      )}
    </SidePage>
  );
}
