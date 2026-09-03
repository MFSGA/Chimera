import { useClashConfig } from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useMemo } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const CLASH_LOG_LEVEL_OPTIONS = {
  debug: 'Debug',
  info: 'Info',
  warning: 'Warn',
  error: 'Error',
  silent: 'Silent',
} as const;

export type ClashLogLevel = keyof typeof CLASH_LOG_LEVEL_OPTIONS;

/** Keep base runtime-config writes behind one application-facing action boundary. */
export const useClashBaseSettings = () => {
  const { query, upsert } = useClashConfig();

  const allowLan = useMemo(
    () => Boolean(query.data?.['allow-lan']),
    [query.data],
  );
  const ipv6 = useMemo(() => Boolean(query.data?.ipv6), [query.data]);
  const logLevel = useMemo(
    () => (query.data?.['log-level'] as ClashLogLevel | undefined) ?? 'debug',
    [query.data],
  );

  const update = useLockFn(
    async (payload: Parameters<typeof upsert.mutateAsync>[0]) => {
      try {
        await upsert.mutateAsync(payload);
      } catch (error) {
        message(formatError(error), {
          title: m.common_error(),
          kind: 'error',
        });
      }
    },
  );

  return {
    allowLan,
    ipv6,
    logLevel,
    logLevelOptions: CLASH_LOG_LEVEL_OPTIONS,
    isPending: upsert.isPending,
    setAllowLan: (value: boolean) => update({ 'allow-lan': value }),
    setIPv6: (value: boolean) => update({ ipv6: value }),
    setLogLevel: (value: ClashLogLevel) => update({ 'log-level': value }),
  };
};
