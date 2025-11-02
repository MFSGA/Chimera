import { useClashProxies, useProxyMode } from '@chimera/interface';
import { cn, SidePage } from '@chimera/ui';
import { Check } from '@mui/icons-material';
import { ToggleButton, ToggleButtonGroup } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { RefObject, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { NodeList, NodeListRef } from '@/components/proxies';
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

  const nodeListRef = useRef<NodeListRef>(null);

  const handleSwitch = useLockFn(async (key: string) => {
    await upsert(key);
  });

  const Header = useMemo(() => {
    return (
      <div className="flex items-center gap-1">
        <ToggleButtonGroup
          color="primary"
          size="small"
          exclusive
          onChange={(_, newValue) => handleSwitch(newValue)}
        >
          {Object.entries(proxyMode).map(([key, enabled], index) => (
            <ToggleButton
              key={key}
              className={cn(
                'flex justify-center gap-0.5 !px-3',
                index === 0 && '!rounded-l-full',
                index === Object.entries(proxyMode).length - 1 &&
                  '!rounded-r-full',
              )}
              value={key}
              selected={enabled}
            >
              {enabled && <Check className="-ml-2 mr-[0.1rem] scale-75" />}
              {t(key)}
            </ToggleButton>
          ))}
        </ToggleButtonGroup>
      </div>
    );
  }, [handleSwitch, proxyMode, t]);

  console.log(data);

  return (
    <SidePage
      title={t('Proxy Groups')}
      leftViewportRef={leftViewportRef}
      rightViewportRef={rightViewportRef}
      header={Header}
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
            <NodeList
              ref={nodeListRef}
              scrollRef={rightViewportRef as RefObject<HTMLElement>}
            />

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
