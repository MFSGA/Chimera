/** A pure cleanup decision: never switch a profile unless the active one is test-owned. */
export function planSubscriptionProfileCleanup(
  originalCurrent: string | null | undefined,
  current: string | null,
  items: readonly { uid: string; name: string }[],
  fixtureName: string,
) {
  if (originalCurrent === undefined) {
    return {
      fixtureUids: [] as string[],
      restoreTo: undefined as string | null | undefined,
      originalMissing: false,
    };
  }

  const fixtureUids = items
    .filter((item) => item.name === fixtureName)
    .map((item) => item.uid);
  const fixtureIsActive = current !== null && fixtureUids.includes(current);
  const originalMissing =
    fixtureIsActive &&
    originalCurrent !== null &&
    !items.some((item) => item.uid === originalCurrent);

  return {
    fixtureUids,
    restoreTo:
      fixtureIsActive && !originalMissing ? originalCurrent : undefined,
    originalMissing,
  };
}
