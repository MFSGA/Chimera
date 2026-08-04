import { mergeFilteredProfileOrder } from './profile-order';

export async function runProfileAction<T>(
  execute: () => Promise<T>,
  reportError: (error: unknown) => void,
): Promise<T | undefined> {
  try {
    return await execute();
  } catch (error) {
    reportError(error);
    return undefined;
  }
}

export async function runProfileOrderAction<T>(
  allUids: readonly string[],
  filteredUids: readonly string[],
  nextFilteredUids: readonly string[],
  submit: (fullOrder: string[]) => Promise<T>,
  reportError: (error: unknown) => void,
): Promise<T | undefined> {
  return runProfileAction(
    async () =>
      submit(
        mergeFilteredProfileOrder(allUids, filteredUids, nextFilteredUids),
      ),
    reportError,
  );
}
