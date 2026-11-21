import {
  MAX_CONNECTIONS_HISTORY,
  MAX_MEMORY_HISTORY,
  MAX_TRAFFIC_HISTORY,
  useClashConnections,
  useClashMemory,
  useClashTraffic,
} from '@chimera/interface';
import { Card, CardContent } from '@chimera/ui/card';
import { useDndGridContext } from '@chimera/ui/dnd-grid';
import { Sparkline } from '@chimera/ui/sparkline';
import { cn } from '@chimera/utils';
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import MemoryOutlineRounded from '~icons/material-symbols/memory-outline-rounded';
import SettingsEthernetRounded from '~icons/material-symbols/settings-ethernet-rounded';
import type { ComponentProps, ReactNode } from 'react';
import * as m from '@/paraglide/messages';
import { formatBytes } from '@/utils/format-bytes';
import type { WidgetComponentProps } from './consts';
import { useWidgetConfig } from './provider';
import { WidgetId } from './widget-config';
import WidgetItem, { type WidgetItemProps } from './widget-item';
import {
  connectionCount,
  connectionSamples,
  padSparklineData,
  trafficSamples,
} from './widget-sparkline-model';
import { WidgetMeta, WidgetMetric, WidgetTitle } from './widget-ui';

function SparklineCard({
  id,
  minH = 2,
  minW = 2,
  maxW,
  maxH,
  chart,
  className,
  children,
  onCloseClick,
  widgetType,
  ...props
}: ComponentProps<typeof Card> & {
  chart: ReactNode;
} & WidgetItemProps) {
  return (
    <WidgetItem
      id={id}
      widgetType={widgetType}
      minH={minH}
      minW={minW}
      maxW={maxW}
      maxH={maxH}
      onCloseClick={onCloseClick}
    >
      <Card
        className={cn('relative isolate size-full', className)}
        data-slot="widget-sparkline-card"
        data-widget-type={widgetType}
        data-widget-id={id}
        {...props}
      >
        {chart}

        <CardContent
          className="relative z-10 flex size-full flex-col justify-between"
          data-slot="widget-sparkline-card-content"
        >
          {children}
        </CardContent>
      </Card>
    </WidgetItem>
  );
}

// Samples arrive several times a second. Only these leaves read them, so a
// sample re-renders the chart and the figures, not the widget around them.

const chartClass = 'absolute inset-0 z-0';
const previewDownloadTraffic = [
  180_000, 240_000, 210_000, 420_000, 360_000, 510_000, 460_000, 720_000,
];
const previewUploadTraffic = [
  45_000, 60_000, 52_000, 90_000, 78_000, 120_000, 135_000, 180_000,
];
const previewConnections = [4, 6, 5, 8, 7, 10, 9, 12];
const previewMemory = [420, 448, 462, 456, 492, 510, 536, 560].map(
  (mebibytes) => mebibytes * 1024 * 1024,
);

function TrafficPreview({
  direction,
  unit,
  showTotal,
}: {
  direction: 'up' | 'down';
  unit: 'bytes' | 'bits';
  showTotal: boolean;
}) {
  const latest = direction === 'down' ? 720_000 : 180_000;
  const total = direction === 'down' ? 86_400_000 : 12_800_000;

  return (
    <>
      <WidgetTitle
        data-slot="widget-sparkline-card-title"
        icon={direction === 'down' ? ArrowDownwardRounded : ArrowUpwardRounded}
      >
        {direction === 'down'
          ? m.dashboard_widget_traffic_download()
          : m.dashboard_widget_traffic_upload()}
      </WidgetTitle>
      <WidgetMetric data-slot="widget-sparkline-card-content">
        {formatBytes(latest, {
          standard: unit === 'bits' ? 'si' : 'iec',
          bits: unit === 'bits',
        })}
        /s
      </WidgetMetric>
      {showTotal && (
        <WidgetMeta data-slot="widget-sparkline-card-bottom">
          {m.dashboard_widget_traffic_total({
            value: formatBytes(total),
          })}
        </WidgetMeta>
      )}
    </>
  );
}

function TrafficChart({ direction }: { direction: 'up' | 'down' }) {
  const { data: clashTraffic } = useClashTraffic();

  return (
    <Sparkline
      data={trafficSamples(clashTraffic, direction, MAX_TRAFFIC_HISTORY)}
      className={chartClass}
    />
  );
}

function TrafficRate({
  direction,
  unit,
}: {
  direction: 'up' | 'down';
  unit: 'bytes' | 'bits';
}) {
  const { data: clashTraffic } = useClashTraffic();

  return (
    <>
      {formatBytes(clashTraffic?.at(-1)?.[direction] ?? 0, {
        standard: unit === 'bits' ? 'si' : 'iec',
        bits: unit === 'bits',
      })}
      /s
    </>
  );
}

function TrafficTotal({ field }: { field: 'downloadTotal' | 'uploadTotal' }) {
  const { data: clashConnections } = useClashConnections();

  const total = clashConnections?.at(-1)?.[field];

  return (
    total !== undefined &&
    m.dashboard_widget_traffic_total({
      value: formatBytes(total),
    })
  );
}

export function TrafficDownWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.TrafficDown);
  const { sourceOnly, isOverlay } = useDndGridContext();

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.TrafficDown}
        chart={
          config.showChart && (
            <Sparkline data={previewDownloadTraffic} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <TrafficPreview
          direction="down"
          unit={config.unit}
          showTotal={config.showTotal}
        />
      </SparklineCard>
    );
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficDown}
      chart={config.showChart && <TrafficChart direction="down" />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle
        data-slot="widget-sparkline-card-title"
        icon={ArrowDownwardRounded}
      >
        {m.dashboard_widget_traffic_download()}
      </WidgetTitle>

      <WidgetMetric data-slot="widget-sparkline-card-content">
        <TrafficRate direction="down" unit={config.unit} />
      </WidgetMetric>

      {config.showTotal && (
        <WidgetMeta data-slot="widget-sparkline-card-bottom">
          <TrafficTotal field="downloadTotal" />
        </WidgetMeta>
      )}
    </SparklineCard>
  );
}

export function TrafficUpWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.TrafficUp);
  const { sourceOnly, isOverlay } = useDndGridContext();

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.TrafficUp}
        chart={
          config.showChart && (
            <Sparkline data={previewUploadTraffic} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <TrafficPreview
          direction="up"
          unit={config.unit}
          showTotal={config.showTotal}
        />
      </SparklineCard>
    );
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.TrafficUp}
      chart={config.showChart && <TrafficChart direction="up" />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle
        data-slot="widget-sparkline-card-title"
        icon={ArrowUpwardRounded}
      >
        {m.dashboard_widget_traffic_upload()}
      </WidgetTitle>

      <WidgetMetric data-slot="widget-sparkline-card-content">
        <TrafficRate direction="up" unit={config.unit} />
      </WidgetMetric>

      {config.showTotal && (
        <WidgetMeta data-slot="widget-sparkline-card-bottom">
          <TrafficTotal field="uploadTotal" />
        </WidgetMeta>
      )}
    </SparklineCard>
  );
}

function ConnectionsChart({ samples }: { samples: number }) {
  const { data: clashConnections } = useClashConnections();

  return (
    <Sparkline
      data={connectionSamples(
        clashConnections,
        Math.min(MAX_CONNECTIONS_HISTORY, samples),
      )}
      className={chartClass}
    />
  );
}

function ConnectionsCount() {
  const { data: clashConnections } = useClashConnections();

  return <>{connectionCount(clashConnections?.at(-1))}</>;
}

export function ConnectionsWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.Connections);
  const { sourceOnly, isOverlay } = useDndGridContext();

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.Connections}
        chart={
          config.showChart && (
            <Sparkline data={previewConnections} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <WidgetTitle
          data-slot="widget-sparkline-card-title"
          icon={SettingsEthernetRounded}
        >
          {m.dashboard_widget_connections()}
        </WidgetTitle>
        <WidgetMetric data-slot="widget-sparkline-card-content">
          12
        </WidgetMetric>
        <WidgetMeta data-slot="widget-sparkline-card-bottom" />
      </SparklineCard>
    );
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Connections}
      chart={config.showChart && <ConnectionsChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle
        data-slot="widget-sparkline-card-title"
        icon={SettingsEthernetRounded}
      >
        {m.dashboard_widget_connections()}
      </WidgetTitle>

      <WidgetMetric data-slot="widget-sparkline-card-content">
        <ConnectionsCount />
      </WidgetMetric>

      <WidgetMeta data-slot="widget-sparkline-card-bottom" />
    </SparklineCard>
  );
}

function MemoryChart({ samples }: { samples: number }) {
  const { data: clashMemory } = useClashMemory();

  return (
    <Sparkline
      data={padSparklineData(
        clashMemory?.map((item) => item.inuse),
        Math.min(MAX_MEMORY_HISTORY, samples),
      )}
      className={chartClass}
    />
  );
}

function MemoryInUse() {
  const { data: clashMemory } = useClashMemory();

  return <>{formatBytes(clashMemory?.at(-1)?.inuse ?? 0)}</>;
}

export function MemoryWidget({ id, onCloseClick }: WidgetComponentProps) {
  const config = useWidgetConfig(id, WidgetId.Memory);
  const { sourceOnly, isOverlay } = useDndGridContext();

  if (sourceOnly || isOverlay) {
    return (
      <SparklineCard
        id={id}
        widgetType={WidgetId.Memory}
        chart={
          config.showChart && (
            <Sparkline data={previewMemory} className={chartClass} />
          )
        }
        onCloseClick={onCloseClick}
      >
        <WidgetTitle
          data-slot="widget-sparkline-card-title"
          icon={MemoryOutlineRounded}
        >
          {m.dashboard_widget_memory()}
        </WidgetTitle>
        <WidgetMetric data-slot="widget-sparkline-card-content">
          {formatBytes(previewMemory.at(-1)!)}
        </WidgetMetric>
        <WidgetMeta data-slot="widget-sparkline-card-bottom" />
      </SparklineCard>
    );
  }

  return (
    <SparklineCard
      id={id}
      widgetType={WidgetId.Memory}
      chart={config.showChart && <MemoryChart samples={config.samples} />}
      onCloseClick={onCloseClick}
    >
      <WidgetTitle
        data-slot="widget-sparkline-card-title"
        icon={MemoryOutlineRounded}
      >
        {m.dashboard_widget_memory()}
      </WidgetTitle>

      <WidgetMetric data-slot="widget-sparkline-card-content">
        <MemoryInUse />
      </WidgetMetric>

      <WidgetMeta data-slot="widget-sparkline-card-bottom" />
    </SparklineCard>
  );
}
