export type TransformChainChanges = {
  added: number;
  removed: number;
  reordered: boolean;
};

/** Compare ordered UID lists without allocating intermediate arrays. */
export const sameOrder = (left: readonly string[], right: readonly string[]) =>
  left.length === right.length &&
  left.every((uid, index) => uid === right[index]);

/** Keep the persisted order but ignore UIDs whose transform profile is gone. */
export function serverChainOf(
  transforms: readonly string[],
  candidateUids: readonly string[],
): string[] {
  const known = new Set(candidateUids);
  return transforms.filter((uid) => known.has(uid));
}

/** Candidate refetches remove deleted items but never auto-add new candidates. */
export function retainKnownDraft(
  draft: readonly string[],
  candidateUids: readonly string[],
): string[] {
  const known = new Set(candidateUids);
  return draft.filter((uid) => known.has(uid));
}

/** Summarize membership and relative-order changes against the saved chain. */
export function changesOf(
  draft: readonly string[],
  serverEnabled: readonly string[],
): TransformChainChanges {
  const draftUids = new Set(draft);
  const appliedUids = new Set(serverEnabled);

  return {
    added: draft.filter((uid) => !appliedUids.has(uid)).length,
    removed: serverEnabled.filter((uid) => !draftUids.has(uid)).length,
    reordered: !sameOrder(
      draft.filter((uid) => appliedUids.has(uid)),
      serverEnabled.filter((uid) => draftUids.has(uid)),
    ),
  };
}
