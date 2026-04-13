import { cn, Sparkline } from '@chimera/ui';
import type { SvgIconComponent } from '@mui/icons-material';
import { Paper } from '@mui/material';
import type { FC } from 'react';
import { useTranslation } from 'react-i18next';
import parseTraffic from '@/utils/parse-traffic';

export interface DatalineProps {
  className?: string;
  data: number[];
  icon: SvgIconComponent;
  title: string;
  total?: number;
  type?: 'speed' | 'raw';
  visible?: boolean;
}

export const Dataline: FC<DatalineProps> = ({
  data,
  icon: Icon,
  title,
  total,
  type,
  className,
  visible = true,
}) => {
  const { t } = useTranslation();
  const latestValue = data.at(-1) ?? 0;

  return (
    <Paper className={cn('relative !rounded-3xl', className)}>
      <Sparkline
        data={data}
        className="absolute rounded-3xl"
        {...(visible !== undefined ? { visible } : {})}
      />

      <div className="relative flex h-full min-h-40 flex-col justify-between gap-4 p-4">
        <div className="flex items-center gap-2">
          <Icon />
          <div className="font-bold">{title}</div>
        </div>

        <div className="text-2xl font-bold text-shadow-md">
          {type === 'raw' ? latestValue : parseTraffic(latestValue).join(' ')}
          {type === 'speed' && '/s'}
        </div>

        <div className="h-5">
          {total !== undefined && (
            <span className="text-shadow-sm">
              {t('Total')}: {parseTraffic(total).join(' ')}
            </span>
          )}
        </div>
      </div>
    </Paper>
  );
};

export default Dataline;
