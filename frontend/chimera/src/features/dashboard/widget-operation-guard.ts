/** Multiple dashboard Widget instances may show the same backend resource.
 * Serialize operations by resource synchronously, before React Query's
 * pending mutation snapshot reaches the next render. */
export function createWidgetOperationGuard() {
  const running = new Set<string>();

  return {
    isPending(key: string): boolean {
      return running.has(key);
    },
    async run(key: string, operation: () => Promise<void>): Promise<boolean> {
      if (running.has(key)) return false;
      running.add(key);
      try {
        await operation();
        return true;
      } finally {
        running.delete(key);
      }
    },
  };
}

export const providerUpdateGuard = createWidgetOperationGuard();

/** Subscription Quota and Schedule widgets can target the same profile.
 * A refresh and its uncertain-result reconciliation are one operation. */
export const profileRefreshGuard = createWidgetOperationGuard();
