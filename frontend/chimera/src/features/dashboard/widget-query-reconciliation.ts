/** A refetch promise can resolve to an error result without rejecting.
 * Stale cached data is not sufficient evidence that a recheck succeeded. */
export function isWidgetQueryReconciled(result: {
  isError: boolean;
  data: unknown;
}): boolean {
  return !result.isError && result.data != null;
}
