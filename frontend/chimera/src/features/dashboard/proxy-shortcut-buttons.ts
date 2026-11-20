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
