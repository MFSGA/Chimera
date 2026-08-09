import { useProxyMode, type ProxyMode } from '@chimera/interface';
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs';
import * as m from '@/paraglide/messages';

export default function ProxyModeSelector() {
  const { value, upsert } = useProxyMode();
  const labels = {
    global: m.settings_system_proxy_global_mode_label(),
    direct: m.settings_system_proxy_direct_mode_label(),
    rule: m.settings_system_proxy_rule_mode_label(),
    script: m.settings_system_proxy_script_mode_label(),
  } satisfies Record<ProxyMode, string>;
  const selectedMode = Object.entries(value).find(
    ([, enabled]) => enabled,
  )?.[0];

  return (
    <AnimatedTabs
      className="h-14 w-full"
      activeTab={selectedMode}
      onChange={(mode) => void upsert(mode as ProxyMode)}
      variant="pill"
    >
      {Object.keys(value).map((mode) => (
        <AnimatedTabsItem key={mode} className="font-semibold" value={mode}>
          {labels[mode as ProxyMode]}
        </AnimatedTabsItem>
      ))}
    </AnimatedTabs>
  );
}
