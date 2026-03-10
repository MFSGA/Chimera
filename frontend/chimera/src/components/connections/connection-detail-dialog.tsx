import { Connection } from '@chimera/interface';
import { BaseDialog, BaseDialogProps, cn } from '@chimera/ui';
import { Tooltip } from '@mui/material';
import { sentenceCase } from 'change-case';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

export type ConnectionDetailDialogProps = { item?: Connection.Item } & Omit<
  BaseDialogProps,
  'title'
>;

const isDisplayValue = (value: unknown) => {
  if (Array.isArray(value)) return value.length > 0;
  return Boolean(value);
};

const formatValue = (key: string, value: unknown): React.ReactElement => {
  if (Array.isArray(value)) {
    return <span>{value.join(' / ')}</span>;
  }

  const normalizedKey = key.toLowerCase();

  if (typeof value === 'number') {
    if (normalizedKey.includes('speed')) {
      return <span>{filesize(value)}/s</span>;
    }

    if (
      normalizedKey.includes('download') ||
      normalizedKey.includes('upload')
    ) {
      return <span>{filesize(value)}</span>;
    }
  }

  if (typeof value === 'string') {
    if (
      normalizedKey.includes('port') ||
      normalizedKey.includes('id') ||
      normalizedKey.includes('ip')
    ) {
      return <span>{value}</span>;
    }

    const date = dayjs(value);

    if (date.isValid()) {
      return (
        <Tooltip title={date.format('YYYY-MM-DD HH:mm:ss')}>
          <span>{date.fromNow()}</span>
        </Tooltip>
      );
    }
  }

  return <span>{String(value)}</span>;
};

function Row({ label, value }: { label: string; value: unknown }) {
  const key = label.toLowerCase();

  return (
    <>
      <div className="w-fit font-bold">{sentenceCase(label)}</div>
      <div
        className={cn(
          'overflow',
          (key === 'id' ||
            key.includes('ip') ||
            key.includes('port') ||
            key.includes('destination') ||
            key.includes('path')) &&
            'font-mono',
        )}
      >
        {formatValue(key, value)}
      </div>
    </>
  );
}

export default function ConnectionDetailDialog({
  item,
  ...others
}: ConnectionDetailDialogProps) {
  const { t } = useTranslation();
  if (!item) return null;

  return (
    <BaseDialog {...others} title={t('Connection Detail')}>
      <div className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-2 px-3">
        {Object.entries(item)
          .filter(([key, value]) => key !== 'metadata' && isDisplayValue(value))
          .map(([key, value]) => (
            <Row key={key} label={key} value={value} />
          ))}

        <h3 className="col-span-2 py-1 pt-5 text-xl font-semibold">
          {t('Metadata')}
        </h3>

        {Object.entries(item.metadata)
          .filter(([, value]) => isDisplayValue(value))
          .map(([key, value]) => (
            <Row key={key} label={key} value={value} />
          ))}
      </div>
    </BaseDialog>
  );
}
