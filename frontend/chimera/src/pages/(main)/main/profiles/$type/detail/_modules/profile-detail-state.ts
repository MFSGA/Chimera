export type ProfileDetailState<T> =
  | { status: 'loading' }
  | { status: 'missing' }
  | { status: 'found'; profile: T };

export function resolveProfileDetailState<T extends { uid: string }>(
  items: readonly T[] | undefined,
  uid: string,
  isPending: boolean,
): ProfileDetailState<T> {
  if (isPending && items === undefined) return { status: 'loading' };

  const profile = items?.find((item) => item.uid === uid);
  return profile ? { status: 'found', profile } : { status: 'missing' };
}
