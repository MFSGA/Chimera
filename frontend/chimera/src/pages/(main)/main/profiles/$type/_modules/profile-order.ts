function assertUnique(values: readonly string[], label: string): Set<string> {
  const unique = new Set(values);
  if (unique.size !== values.length) {
    throw new Error(`${label} contains duplicate profile identifiers`);
  }
  return unique;
}

export function profileOrderChanged(
  previous: readonly string[],
  next: readonly string[],
): boolean {
  return (
    previous.length !== next.length ||
    previous.some((uid, index) => uid !== next[index])
  );
}

export function mergeFilteredProfileOrder(
  allUids: readonly string[],
  filteredUids: readonly string[],
  nextFilteredUids: readonly string[],
): string[] {
  const allSet = assertUnique(allUids, 'full profile order');
  const filteredSet = assertUnique(filteredUids, 'filtered profile order');
  const nextSet = assertUnique(nextFilteredUids, 'next filtered profile order');

  if (filteredUids.length !== nextFilteredUids.length) {
    throw new Error('filtered profile order length changed during reordering');
  }
  for (const uid of filteredSet) {
    if (!allSet.has(uid)) {
      throw new Error(`filtered profile ${uid} is missing from the full order`);
    }
    if (!nextSet.has(uid)) {
      throw new Error(`next filtered order is missing profile ${uid}`);
    }
  }
  for (const uid of nextSet) {
    if (!filteredSet.has(uid)) {
      throw new Error(`next filtered order contains unexpected profile ${uid}`);
    }
  }

  let cursor = 0;
  return allUids.map((uid) =>
    filteredSet.has(uid) ? nextFilteredUids[cursor++] : uid,
  );
}
