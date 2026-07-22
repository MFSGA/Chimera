import {
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
  useSetting,
  type ClashMemory,
  type ClashTraffic,
} from '@chimera/interface';
import {
  ArrowDownward,
  ArrowUpward,
  MemoryOutlined,
  SettingsEthernet,
} from '@mui/icons-material';
import Grid from '@mui/material/Grid';
import { useAtomValue } from 'jotai';
import { useMemo } from 'react';
import * as m from '@/paraglide/messages';
import { atomIsDrawer } from '@/store';
import Dataline, { type DatalineProps } from './dataline';

export const DataPanel = ({ visible = true }: { visible?: boolean }) => {
  const { data: clashTraffic } = useClashTraffic();
  const { data: clashMemory } = useClashMemory();
  const { data: clashConnections } = useClashConnections();
  const { value } = useSetting('clash_core');

  const supportMemory = Boolean(
    value && ['mihomo', 'mihomo-alpha'].includes(value),
  );

  const padData = (data: (number | undefined)[] = [], max: number) =>
    Array(Math.max(0, max - data.length))
      .fill(0)
      .concat(data.slice(-max));

  const datalines: (DatalineProps & { visible?: boolean })[] = [
    {
      data: padData(
        clashTraffic?.map((item: ClashTraffic) => item.down),
        MAX_TRAFFIC_HISTORY,
      ),
      icon: ArrowDownward,
      title: m.dashboard_widget_traffic_download(),
      total: clashConnections?.at(-1)?.downloadTotal,
      type: 'speed',
      visible,
    },
    {
      data: padData(
        clashTraffic?.map((item: ClashTraffic) => item.up),
        MAX_TRAFFIC_HISTORY,
      ),
      icon: ArrowUpward,
      title: m.dashboard_widget_traffic_upload(),
      total: clashConnections?.at(-1)?.uploadTotal,
      type: 'speed',
      visible,
    },
    {
      data: padData(
        clashConnections?.map((item) => item.connections?.length ?? 0),
        MAX_CONNECTIONS_HISTORY,
      ),
      icon: SettingsEthernet,
      title: m.dashboard_widget_connections(),
      type: 'raw',
      visible,
    },
  ];

  if (supportMemory) {
    datalines.splice(2, 0, {
      data: padData(
        clashMemory?.map((item: ClashMemory) => item.inuse),
        MAX_MEMORY_HISTORY,
      ),
      icon: MemoryOutlined,
      title: m.dashboard_widget_memory(),
      visible,
    });
  }

  const isDrawer = useAtomValue(atomIsDrawer);

  const gridLayout = useMemo(
    () => ({
      sm: isDrawer ? 6 : 12,
      md: 6,
      lg: supportMemory ? 3 : 4,
      xl: supportMemory ? 3 : 4,
    }),
    [isDrawer, supportMemory],
  );

  return datalines.map((props, index) => (
    <Grid key={`data-${index}`} size={gridLayout}>
      <Dataline {...props} className="min-h-40" />
    </Grid>
  ));
};

export default DataPanel;
