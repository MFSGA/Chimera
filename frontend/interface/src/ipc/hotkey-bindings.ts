import { mutationOptions, queryOptions } from '@tanstack/react-query';
import { commands } from './bindings.js';

export const queries = {
  getHotkeyFunctions: (
    ...args: Parameters<typeof commands.getHotkeyFunctions>
  ) =>
    queryOptions({
      queryKey: ['getHotkeyFunctions', ...args],
      queryFn: () => commands.getHotkeyFunctions(...args),
    }),
  getHotkeys: (...args: Parameters<typeof commands.getHotkeys>) =>
    queryOptions({
      queryKey: ['getHotkeys', ...args],
      queryFn: () => commands.getHotkeys(...args),
    }),
};

export const mutations = {
  setHotkeys: mutationOptions({
    mutationKey: ['setHotkeys'],
    mutationFn: (input: Parameters<typeof commands.setHotkeys>) =>
      commands.setHotkeys(...input),
  }),
};
