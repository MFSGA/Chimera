import {
  useClashProxiesProvider,
  useClashRulesProvider,
  useProfile,
} from '@chimera/interface';
import type { DashboardItem } from './consts';
import { useDashboardContext } from './provider';
import { selectWidgetCalibrationSources } from './widget-data-calibration-model';

function ProfileCalibration() {
  useProfile({ without_helper_fn: true, refetchInterval: 60_000 });
  return null;
}

function ProviderCalibration({
  proxy,
  rule,
}: {
  proxy: boolean;
  rule: boolean;
}) {
  useClashProxiesProvider({ refetchInterval: 60_000, enabled: proxy });
  useClashRulesProvider({ refetchInterval: 60_000, enabled: rule });
  return null;
}

/** One low-frequency observer for each data source used by visible cards. */
export function WidgetDataCalibration({ items }: { items: DashboardItem[] }) {
  const { configs, configLoading, configReadError } = useDashboardContext();
  if (configLoading || configReadError) return null;
  const { profiles, proxy, rule } = selectWidgetCalibrationSources(
    items,
    configs,
  );
  return (
    <>
      {profiles && <ProfileCalibration />}
      {(proxy || rule) && <ProviderCalibration proxy={proxy} rule={rule} />}
    </>
  );
}
