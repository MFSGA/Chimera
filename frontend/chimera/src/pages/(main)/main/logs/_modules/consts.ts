export enum LogLevel {
  Trace = 'trace',
  Debug = 'debug',
  Info = 'info',
  Warning = 'warning',
  Error = 'error',
  Fatal = 'fatal',
  Unknown = 'unknown',
}

// The historical WS adapter may emit `warn` while navigation uses `warning`.
export function matchesLogLevel(
  recordLevel: string,
  selected: LogLevel | null | undefined,
): boolean {
  if (!selected) return true;
  const normalized = recordLevel.toLowerCase();
  return selected === LogLevel.Warning
    ? normalized === 'warn' || normalized === 'warning'
    : normalized === selected;
}
