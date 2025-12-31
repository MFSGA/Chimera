import { Connection } from '@chimera/interface';
import { BaseDialog, BaseDialogProps } from '@chimera/ui';
import { Tooltip } from '@mui/material';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

export type ConnectionDetailDialogProps = { item?: Connection.Item } & Omit<
  BaseDialogProps,
  'title'
>;

export default function ConnectionDetailDialog({
  item,
  ...others
}: ConnectionDetailDialogProps) {
  const { t } = useTranslation();
  if (!item) return null;

  return (
    <BaseDialog {...others} title={t('Connection Detail')}>
      <div className="grid grid-cols-[max-content_1fr] gap-x-3 gap-y-2 px-3">
        hello detail
      </div>
    </BaseDialog>
  );
}
