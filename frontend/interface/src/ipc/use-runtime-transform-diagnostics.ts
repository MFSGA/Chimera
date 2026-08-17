import { useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY } from './consts';

export const useRuntimeTransformDiagnostics = (enabled = true) =>
  useQuery({
    queryKey: [RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY],
    queryFn: async () =>
      unwrapResult(await commands.getRuntimeTransformDiagnostics()),
    enabled,
    staleTime: 0,
  });
