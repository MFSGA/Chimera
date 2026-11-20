/** Shared ref ordering for both the inert drag preview and live controls.
 * The Linux-only capture shortcut never appears on unsupported hosts. */
export type ProxyShortcutButtonId = 'system' | 'tun' | 'transparent';

export function visibleProxyShortcutButtons(
  selected: readonly ProxyShortcutButtonId[],
  order: 'system-first' | 'tun-first',
  linuxDesktop: boolean,
): ProxyShortcutButtonId[] {
  const orderList: ProxyShortcutButtonId[] =
    order === 'system-first'
      ? ['system', 'tun', 'transparent']
      : ['tun', 'system', 'transparent'];
  return orderList.filter(
    (button) =>
      selected.includes(button) && (button !== 'transparent' || linuxDesktop),
  );
}

/** The menu counts only controls available on this host, while leaving
 * the persisted Linux selection intact when a profile opens elsewhere. */
export function visibleSelectedProxyShortcutButtons(
  selected: readonly ProxyShortcutButtonId[],
  linuxDesktop: boolean,
): ProxyShortcutButtonId[] {
  return selected.filter(
    (button, index) =>
      selected.indexOf(button) === index &&
      (button !== 'transparent' || linuxDesktop),
  );
}

export function proxyShortcutSelectionSummary(
  selected: readonly ProxyShortcutButtonId[],
  linuxDesktop: boolean,
  labels: Record<ProxyShortcutButtonId | 'all', string>,
): string {
  const visible = visibleSelectedProxyShortcutButtons(selected, linuxDesktop);
  if (visible.length === (linuxDesktop ? 3 : 2)) return labels.all;
  return visible.map((button) => labels[button]).join(', ') || '—';
}

/** Prevent removing the last visible shortcut, including when Linux-only
 * transparent mode is present in the saved settings but hidden on this host. */
export function toggleProxyShortcutSelection(
  selected: readonly ProxyShortcutButtonId[],
  button: ProxyShortcutButtonId,
  checked: boolean,
  linuxDesktop: boolean,
): ProxyShortcutButtonId[] {
  if (button === 'transparent' && !linuxDesktop) return [...selected];
  if (checked) {
    return selected.includes(button) ? [...selected] : [...selected, button];
  }
  if (
    !selected.includes(button) ||
    visibleSelectedProxyShortcutButtons(selected, linuxDesktop).length <= 1
  ) {
    return [...selected];
  }
  return selected.filter((item) => item !== button);
}
