import { useQuery } from '@tanstack/react-query';
import { queries } from '../ipc/hotkey-bindings.js';

export function useHotkeyFunctions() {
  const options = queries.getHotkeyFunctions();
  return useQuery(options);
}
