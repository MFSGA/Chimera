import { useQuery } from '@tanstack/react-query';
import { unwrapResult } from '../utils';
import { commands } from './bindings';

export const RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY =
  'runtime-transform-diagnostics';

export const useRuntimeTransformDiagnostics = (enabled = true) =>
  useQuery({
    queryKey: [RUNTIME_TRANSFORM_DIAGNOSTICS_QUERY_KEY],
    queryFn: async () =>
      unwrapResult(await commands.getRuntimeTransformDiagnostics()),
    enabled,
    staleTime: 0,
  });
