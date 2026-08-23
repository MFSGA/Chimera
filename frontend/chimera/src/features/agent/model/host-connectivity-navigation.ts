type HostConnectivityCard = Pick<HTMLElement, 'focus' | 'scrollIntoView'>;

type HostConnectivityNavigation = {
  refresh: () => Promise<unknown>;
  schedule: (callback: () => void) => unknown;
  findCard: () => HostConnectivityCard | null;
};

/** Refresh diagnostics before focusing the existing privacy-safe connectivity card. */
export async function inspectHostConnectivityCard({
  refresh,
  schedule,
  findCard,
}: HostConnectivityNavigation) {
  await refresh();
  schedule(() => {
    const card = findCard();
    card?.focus();
    card?.scrollIntoView({ behavior: 'smooth', block: 'center' });
  });
}
