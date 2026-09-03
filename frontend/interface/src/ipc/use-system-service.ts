import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { executeServiceMutation } from './service-mutation';

export type ServiceType =
  'install' | 'uninstall' | 'start' | 'stop' | 'restart';

/**
 * Custom hook to fetch and manage the system service status using TanStack Query.
 *
 * @returns An object containing the query result for the system service status.
 */
export const useSystemService = () => {
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: ['system-service'],
    queryFn: async () => {
      return unwrapResult(await commands.statusService());
    },
  });

  const upsert = useMutation({
    mutationFn: async (type: ServiceType) => {
      await executeServiceMutation(commands, type);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['system-service'] });
    },
  });

  return {
    query,
    upsert,
  };
};
