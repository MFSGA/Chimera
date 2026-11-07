import { useQuery } from '@tanstack/react-query';
import { queries } from './query-bindings.js';
import { unwrapQueryOptions } from './query-options.js';

export const useClashVersion = () => {
  const version = queries.clashApiGetVersion();
  return useQuery(unwrapQueryOptions(version, version.queryFn!));
};
