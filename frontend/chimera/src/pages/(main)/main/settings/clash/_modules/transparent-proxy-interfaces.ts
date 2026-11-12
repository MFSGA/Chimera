/** Normalize the comma-separated Linux interface list used by transparent proxy capture. */
export function normalizeTransparentProxyInterfaces(value: string): string[] {
  return [
    ...new Set(
      value
        .split(',')
        .map((item) => item.trim())
        .filter(Boolean),
    ),
  ];
}
