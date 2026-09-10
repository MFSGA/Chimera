import { useClashConnections } from '@chimera/interface';
import { cn } from '@chimera/utils';
import DownloadRounded from '~icons/material-symbols/download-rounded';
import UploadRounded from '~icons/material-symbols/upload-rounded';
import { filesize } from 'filesize';
import { useEffect, useRef, useState } from 'react';

function TrafficChip({
  direction,
  value,
  loading,
  highlighted,
}: {
  direction: 'download' | 'upload';
  value?: number;
  loading: boolean;
  highlighted: boolean;
}) {
  const Icon = direction === 'download' ? DownloadRounded : UploadRounded;

  return (
    <div className="bg-surface-variant/70 flex min-h-8 items-center justify-center gap-1 rounded-full px-2">
      <Icon
        className={cn(
          'size-4 transition-colors',
          highlighted ? 'text-primary' : 'text-on-surface-variant',
        )}
      />
      {loading ? (
        <span className="bg-on-surface-variant/20 h-3 w-14 animate-pulse rounded-full" />
      ) : (
        <span className="font-mono text-xs">
          {filesize(value ?? 0, { pad: true })}
        </span>
      )}
    </div>
  );
}

export default function ConnectionsTotal() {
  const { data: clashConnections, isLoading } = useClashConnections();
  const latestClashConnections = clashConnections?.at(-1);
  const [downloadHighlight, setDownloadHighlight] = useState(false);
  const [uploadHighlight, setUploadHighlight] = useState(false);
  const downloadHighlightTimerRef = useRef<number | null>(null);
  const uploadHighlightTimerRef = useRef<number | null>(null);

  useEffect(() => {
    if ((latestClashConnections?.downloadTotal ?? 0) <= 0) return;

    setDownloadHighlight(true);
    if (downloadHighlightTimerRef.current) {
      clearTimeout(downloadHighlightTimerRef.current);
    }
    downloadHighlightTimerRef.current = window.setTimeout(() => {
      setDownloadHighlight(false);
    }, 300);
  }, [latestClashConnections?.downloadTotal]);

  useEffect(() => {
    if ((latestClashConnections?.uploadTotal ?? 0) <= 0) return;

    setUploadHighlight(true);
    if (uploadHighlightTimerRef.current) {
      clearTimeout(uploadHighlightTimerRef.current);
    }
    uploadHighlightTimerRef.current = window.setTimeout(() => {
      setUploadHighlight(false);
    }, 300);
  }, [latestClashConnections?.uploadTotal]);

  const loading = isLoading || !latestClashConnections;

  return (
    <div className="flex gap-2">
      <TrafficChip
        direction="download"
        value={latestClashConnections?.downloadTotal}
        loading={loading}
        highlighted={downloadHighlight}
      />
      <TrafficChip
        direction="upload"
        value={latestClashConnections?.uploadTotal}
        loading={loading}
        highlighted={uploadHighlight}
      />
    </div>
  );
}
