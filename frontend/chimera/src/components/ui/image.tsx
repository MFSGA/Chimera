import { useServerPort } from '@chimera/interface';
import { LazyImage, type LazyImageProps } from '@chimera/ui';
import { useMemo } from 'react';

type SharedImageProps = Omit<LazyImageProps, 'src'>;

export function CacheImage({
  icon,
  ...props
}: SharedImageProps & {
  icon: string;
}) {
  const { data: serverPort } = useServerPort();

  const src = icon.trim().startsWith('<svg')
    ? `data:image/svg+xml;base64,${btoa(icon)}`
    : icon;

  const cachedUrl = useMemo(() => {
    if (!src.startsWith('http') || !serverPort) {
      return src;
    }

    return `http://localhost:${serverPort}/cache/icon?url=${btoa(src)}`;
  }, [src, serverPort]);

  return <LazyImage src={cachedUrl} {...props} />;
}

export function TrayImage({
  mode,
  version,
  ...props
}: SharedImageProps & {
  mode: 'system_proxy' | 'tun' | 'normal';
  version?: number;
}) {
  const { data: serverPort } = useServerPort();

  const src = serverPort
    ? `http://localhost:${serverPort}/tray/icon?mode=${mode}${version !== undefined ? `&v=${version}` : ''}`
    : '';

  return <LazyImage src={src} {...props} />;
}
