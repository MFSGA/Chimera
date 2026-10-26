type TaggedRelease = { tag_name: string };

function isNotFoundError(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'status' in error &&
    error.status === 404
  );
}

export async function resolveReleaseByTag<T extends TaggedRelease>(
  tag: string,
  getByTag: () => Promise<T>,
  listReleases: () => Promise<readonly T[]>,
): Promise<T> {
  try {
    return await getByTag();
  } catch (error) {
    if (!isNotFoundError(error)) throw error;

    // GitHub's by-tag endpoint omits drafts, while the release list includes them.
    const release = (await listReleases()).find(
      (candidate) => candidate.tag_name === tag,
    );
    if (release) return release;

    throw error;
  }
}
