import { useSystemProxy } from '@chimera/interface';
import * as m from '@/paraglide/messages';

export default function CurrentSystemProxy() {
  const { data } = useSystemProxy();
  const entries = Object.entries(data ?? {});

  if (entries.length === 0) {
    return (
      <div className="leading-8">
        {m.settings_system_proxy_no_proxy_label()}
      </div>
    );
  }

  return (
    <div
      data-slot="current-system-proxy-container"
      className="flex flex-col gap-0.5 select-text"
    >
      {entries.map(([key, value]) => (
        <div key={key} className="flex w-full gap-4 leading-8">
          <div className="w-28 shrink-0 capitalize">{key}:</div>
          <div className="min-w-0 flex-1 break-all">{String(value)}</div>
        </div>
      ))}
    </div>
  );
}
