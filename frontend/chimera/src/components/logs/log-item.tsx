import { type ClashLog } from '@chimera/interface';
import { cn } from '@chimera/ui';
import {
  Box,
  useColorScheme,
  type CSSObject,
  type SxProps,
  type Theme,
} from '@mui/material';
import { useAsyncEffect } from 'ahooks';
import { useState, type CSSProperties } from 'react';
import { formatAnsi } from '@/utils/shiki';
import styles from './log-item.module.scss';

const logLevelStyles: Record<string, SxProps<Theme>> = {
  err: (theme): CSSObject => ({
    color: theme.vars.palette.error.main,
  }),
  warn: (theme): CSSObject => ({
    color: theme.vars.palette.warning.main,
  }),
  inf: (theme): CSSObject => ({
    color: theme.vars.palette.info.main,
  }),
};

export const LogItem = ({
  value,
  className,
}: {
  value: ClashLog;
  className?: string;
}) => {
  const [payload, setPayload] = useState(value.payload);
  const { mode } = useColorScheme();

  useAsyncEffect(async () => {
    setPayload(await formatAnsi(value.payload));
  }, [value.payload]);

  return (
    <div
      className={cn(
        'w-full rounded-2xl px-5 py-3 font-mono transition-colors select-text',
        className,
      )}
    >
      <div className="mb-2 flex items-center gap-2 text-xs tracking-[0.16em] text-zinc-500 uppercase dark:text-zinc-400">
        <span className="font-light">{value.time ?? '--:--:--'}</span>

        <Box
          component="span"
          className="inline-flex rounded-full bg-black/5 px-2 py-0.5 font-semibold dark:bg-white/8"
          sx={logLevelStyles[value.type]}
        >
          {value.type}
        </Box>
      </div>

      <div className="pb-1 text-wrap">
        <div
          className={cn(styles.item, mode === 'dark' && styles.dark)}
          style={
            {
              '--item-font': 'var(--font-mono)',
            } as CSSProperties
          }
          dangerouslySetInnerHTML={{
            __html: payload,
          }}
        />
      </div>
    </div>
  );
};

export default LogItem;
