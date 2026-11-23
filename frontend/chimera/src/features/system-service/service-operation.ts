import type { ServiceType } from '@chimera/interface';

export type ManualPromptOperation = Exclude<ServiceType, 'restart'>;

export type ServiceOperationResult =
  | { stage: 'completed' }
  | { stage: 'service_failed'; error: unknown }
  | { stage: 'core_restart_failed'; error: unknown };

/** The OS Service and ref CoreActor are distinct lifecycle authorities.
 * A Service command may succeed even if the subsequent Core reconcile
 * fails. Only actual Service command failures may ask for manual elevation.
 */
export async function runServiceOperation(
  operation: ManualPromptOperation,
  performService: (operation: ManualPromptOperation) => Promise<unknown>,
  restartCore: () => Promise<unknown>,
): Promise<ServiceOperationResult> {
  try {
    await performService(operation);
  } catch (error) {
    return { stage: 'service_failed', error };
  }
  try {
    await restartCore();
  } catch (error) {
    return { stage: 'core_restart_failed', error };
  }
  return { stage: 'completed' };
}
