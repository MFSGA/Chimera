import { useClashProxies, useProxyMode } from '@chimera/interface';
import { SidePage } from '@chimera/ui';
import { createFileRoute } from '@tanstack/react-router';
import { RefObject, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { GroupList } from '@/components/proxies/group-list';

export const Route = createFileRoute('/proxies')({
  component: ProxyPage,
});

function ProxyPage() {
  const { t } = useTranslation();

  const { value: proxyMode, upsert } = useProxyMode();

  const { data } = useClashProxies();

  const hasProxies = Boolean(data?.groups.length);

  const leftViewportRef = useRef<HTMLDivElement>(null);

  const rightViewportRef = useRef<HTMLDivElement>(null);

  console.log(data);

  return (
    <SidePage
      title={t('Proxy Groups')}
      leftViewportRef={leftViewportRef}
      rightViewportRef={rightViewportRef}
      side={
        hasProxies &&
        proxyMode.rule && (
          <GroupList scrollRef={leftViewportRef as RefObject<HTMLElement>} />
        )
      }
    >
      {!proxyMode.direct ? (
        hasProxies ? (
          <>
            <div>hasProxies</div>
            {/* <NodeList
              ref={nodeListRef}
              scrollRef={rightViewportRef as RefObject<HTMLElement>}
            /> */}

            {/*<DelayButton onClick={handleDelayClick} /> */}
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
