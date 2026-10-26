import { useKvStorage } from '@chimera/interface';
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  type PropsWithChildren,
} from 'react';
import * as m from '@/paraglide/messages';
import { message } from '@/utils/notification';
import { RANKINGS, type RankingDimension } from './traffic-context';
import {
  FLOW_GRAPHS,
  normalizeTrafficViewSettings,
  TRAFFIC_VIEW_KV_KEY,
  type FlowGraph,
  type TrafficViewSettings,
} from './view-settings-model';

export {
  FLOW_GRAPHS,
  TRAFFIC_VIEW_KV_KEY,
  type FlowGraph,
  type TrafficViewSettings,
} from './view-settings-model';

export const DEFAULT_TRAFFIC_VIEW: TrafficViewSettings<RankingDimension> = {
  // The most asked-for rankings.
  rankings: RANKINGS.slice(0, 3).map(({ dimension }) => dimension),
  flowGraph: 'sankey',
};

/** Coerces whatever the KV store holds into the view settings. */
export function toTrafficViewSettings(
  value: unknown,
): TrafficViewSettings<RankingDimension> {
  return normalizeTrafficViewSettings(
    value,
    RANKINGS.map(({ dimension }) => dimension),
    DEFAULT_TRAFFIC_VIEW,
  );
}

// Apart from the page context, so a change re-renders only what reads it.
const TrafficViewSettingsContext = createContext<{
  settings: TrafficViewSettings<RankingDimension>;
  onSettingsChange: (
    update: Partial<TrafficViewSettings<RankingDimension>>,
  ) => void;
} | null>(null);

export function useTrafficViewSettings() {
  const context = useContext(TrafficViewSettingsContext);

  if (!context) {
    throw new Error('Traffic pages must render inside the traffic layout');
  }

  return context;
}

/**
 * Reads the view settings once for all the traffic pages, which keep them
 * while they switch. The store's own state stays in here, so reading it
 * leaves the layout alone.
 */
export function TrafficViewSettingsProvider({ children }: PropsWithChildren) {
  const [settings, setSettings] = useKvStorage(
    TRAFFIC_VIEW_KV_KEY,
    DEFAULT_TRAFFIC_VIEW,
    { migrate: toTrafficViewSettings },
  );

  const onSettingsChange = useCallback(
    async (update: Partial<TrafficViewSettings<RankingDimension>>) => {
      if (!(await setSettings((previous) => ({ ...previous, ...update })))) {
        message(m.traffic_view_settings_save_failed(), { kind: 'error' });
      }
    },
    [setSettings],
  );
  const value = useMemo(
    () => ({ settings, onSettingsChange }),
    [settings, onSettingsChange],
  );

  return (
    <TrafficViewSettingsContext value={value}>
      {children}
    </TrafficViewSettingsContext>
  );
}
