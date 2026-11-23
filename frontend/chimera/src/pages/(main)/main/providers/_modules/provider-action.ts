/**
 * Preserve the original failure in useBlockTask even after displaying it.
 *
 * A provider mutation can reject despite the remote update being applied;
 * the Interface mutation invalidates its caches on settlement.  The UI must
 * not mark a rejected mutation as a completed task.
 */
export async function runProviderAction<T>(
  operation: () => Promise<T>,
  reportFailure: (error: unknown) => void | Promise<void>,
): Promise<T> {
  try {
    return await operation();
  } catch (error) {
    try {
      await reportFailure(error);
    } finally {
      throw error;
    }
  }
}
