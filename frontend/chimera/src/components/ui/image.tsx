import { cachedIconUrl, trayIconUrl, widgetIconUrl } from '@chimera/interface';
import { LazyImage, type LazyImageProps } from '@chimera/ui';

type SharedImageProps = Omit<LazyImageProps, 'src'>;

export function CacheImage({
  icon,
  ...props
}: SharedImageProps & {
  icon: string;
}) {
  const src = icon.trim().startsWith('<svg')
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon;

  const imageUrl = src.startsWith('http') ? cachedIconUrl(src) : src;

  return <LazyImage src={imageUrl} {...props} />;
}

export function TrayImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal';
  version?: number;
}) {
  const src = trayIconUrl(mode, version);

  return <LazyImage src={src} {...props} />;
}

export function WidgetImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal';
  version?: number;
}) {
  const src = widgetIconUrl(mode, version);

  return <LazyImage src={src} {...props} />;
}
