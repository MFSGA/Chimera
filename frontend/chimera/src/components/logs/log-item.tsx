import { type ClashLog } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Box, useColorScheme, type SxProps, type Theme } from '@mui/material';
import { useAsyncEffect } from 'ahooks';
import { useState } from 'react';
import { formatAnsi } from '@/utils/shiki';

const logLevelStyles: Record<string, SxProps<Theme>> = {
  err: (theme) => ({
    color: theme.vars.palette.error.main,
  }),
  warn: (theme) => ({
    color: theme.vars.palette.warning.main,
  }),
  inf: (theme) => ({
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
      className={cn('w-full p-4 pt-2 pb-0 font-mono select-text', className)}
    >
      <div className="flex gap-2">
        <span className="font-thin">{value.time}</span>

        <Box
          component="span"
          className="inline-block font-semibold uppercase"
          sx={logLevelStyles[value.type]}
        >
          {value.type}
        </Box>
      </div>

      <div className="pb-2 text-wrap">
        <div
          className={cn(
            '[&_.shiki]:!mb-0 [&_.shiki]:!bg-transparent [&_.shiki_*]:font-mono',
            mode === 'dark' && '[&_.shiki_span]:!text-[var(--shiki-dark)]',
          )}
          dangerouslySetInnerHTML={{
            __html: payload,
          }}
        />
      </div>
    </div>
  );
};

export default LogItem;
